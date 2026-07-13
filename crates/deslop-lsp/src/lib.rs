use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use deslop_analyzer::{
    AnalyzerConfig, AnalyzerLangConfig, RuleSuppression, Suppression,
    scan_analysis_with_presentation,
};
use deslop_core::{
    AnalysisDiagnostic, AnalysisProvenance, FileReport, Finding, SafetyClass, Severity,
};
use deslop_fix::apply_findings_to_text;
use deslop_parse::{ProjectAnalysis, ProjectSnapshotPlanner, SnapshotPresentationMap};
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    Notification as LspNotification, PublishDiagnostics,
};
use lsp_types::request::{CodeActionRequest, Request as LspRequest};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionProviderCapability,
    CodeActionResponse, Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentChanges, NumberOrString, OneOf, OptionalVersionedTextDocumentIdentifier, Position,
    PublishDiagnosticsParams, Range, ServerCapabilities, TextDocumentContentChangeEvent,
    TextDocumentEdit, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
    WorkspaceEdit,
};
use serde::Deserialize;

#[derive(Debug, Clone)]
struct DocumentState {
    text: String,
    findings: Vec<Finding>,
    analysis: AnalysisProvenance,
    project_analysis: Arc<ProjectAnalysis>,
    presentation: SnapshotPresentationMap,
    logical_path: PathBuf,
    path: PathBuf,
    version: Option<i32>,
}

#[derive(Debug, Default)]
struct LspState {
    documents: BTreeMap<Uri, DocumentState>,
    workspace_root: Option<PathBuf>,
    config_path: Option<PathBuf>,
    analyzer_config: AnalyzerConfig,
}

#[derive(Debug, Default, Deserialize)]
struct LspConfig {
    analyzer: Option<LspAnalyzerConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LspAnalyzerConfig {
    #[serde(default)]
    min_duplication_tokens: Option<usize>,
    #[serde(default)]
    long_method_nloc: Option<usize>,
    #[serde(default)]
    min_meaningful_tokens: Option<usize>,
    #[serde(default)]
    disabled_rules: Option<Vec<String>>,
    #[serde(default)]
    ignore_paths: Option<Vec<String>>,
    #[serde(default)]
    rules: Option<BTreeMap<String, LspRuleConfig>>,
    #[serde(default)]
    rust: Option<LspAnalyzerLangConfig>,
    #[serde(default)]
    clojure: Option<LspAnalyzerLangConfig>,
    #[serde(default)]
    julia: Option<LspAnalyzerLangConfig>,
    #[serde(default)]
    python: Option<LspAnalyzerLangConfig>,
    #[serde(default)]
    javascript: Option<LspAnalyzerLangConfig>,
    #[serde(default)]
    typescript: Option<LspAnalyzerLangConfig>,
    #[serde(default)]
    generic: Option<LspAnalyzerLangConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LspRuleConfig {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    ignore_paths: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LspAnalyzerLangConfig {
    #[serde(default)]
    long_method_nloc: Option<usize>,
}

impl LspState {
    fn open(&mut self, uri: Uri, path: PathBuf, text: String, version: Option<i32>) -> Result<()> {
        self.refresh_config_for_path(&path).ok();
        let analyzed = analyze_document(&path, &text, &self.analyzer_config, None)?;
        self.documents.insert(
            uri,
            DocumentState {
                text,
                path,
                findings: analyzed.report.findings,
                analysis: analyzed.report.analysis,
                project_analysis: analyzed.analysis,
                presentation: analyzed.presentation,
                logical_path: analyzed.logical_path,
                version,
            },
        );
        Ok(())
    }

    fn change(
        &mut self,
        uri: Uri,
        changes: Vec<TextDocumentContentChangeEvent>,
        version: Option<i32>,
    ) -> Result<()> {
        let text = self
            .documents
            .get(&uri)
            .map(|document| document.text.to_owned())
            .unwrap_or_default();
        let text = apply_text_document_changes(&text, changes)?;
        let path = self
            .documents
            .get(&uri)
            .map(|document| document.path.to_owned())
            .ok_or_else(|| anyhow::anyhow!("document not open: {:?}", uri))?;
        self.refresh_config_for_path(&path).ok();
        let previous = Arc::clone(&self.documents[&uri].project_analysis);
        let previous_logical = self.documents[&uri].logical_path.clone();
        let analyzed = analyze_document(&path, &text, &self.analyzer_config, Some(&previous))?;
        if analyzed.logical_path != previous_logical {
            bail!("LSP document logical path changed across an incremental revision");
        }
        self.documents.insert(
            uri,
            DocumentState {
                text,
                path,
                findings: analyzed.report.findings,
                analysis: analyzed.report.analysis,
                project_analysis: analyzed.analysis,
                presentation: analyzed.presentation,
                logical_path: analyzed.logical_path,
                version,
            },
        );
        Ok(())
    }

    fn save(&mut self, uri: &Uri, text: Option<String>) -> Result<()> {
        if let Some(text) = text {
            let changes = vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text,
            }];
            self.change(uri.clone(), changes, None)?;
        } else if let Some((path, analysis, presentation)) =
            self.documents.get(uri).map(|document| {
                (
                    document.path.to_owned(),
                    Arc::clone(&document.project_analysis),
                    document.presentation.clone(),
                )
            })
        {
            self.refresh_config_for_path(&path).ok();
            let config = document_analyzer_config(&self.analyzer_config);
            let projection = scan_analysis_with_presentation(analysis, &presentation, config)?;
            if let Some(document) = self.documents.get_mut(uri) {
                let report = projection
                    .reports
                    .into_iter()
                    .find(|report| report.path == path)
                    .ok_or_else(|| anyhow::anyhow!("analyzer returned no LSP report"))?;
                document.findings = report.findings;
                document.analysis = report.analysis;
            }
        }
        Ok(())
    }

    fn close(&mut self, uri: &Uri) {
        self.documents.remove(uri);
    }

    fn refresh_config_for_path(&mut self, path: &Path) -> Result<()> {
        let root = self
            .workspace_root
            .clone()
            .or_else(|| path.parent().map(ToOwned::to_owned));
        let Some(root) = root else {
            return Ok(());
        };
        let next = resolve_config_path(&root);
        if next.as_deref() != self.config_path.as_deref() {
            self.analyzer_config = load_analyzer_config(next.as_deref())?;
            self.config_path = next;
        }
        Ok(())
    }
}

pub fn run_stdio() -> Result<()> {
    let (connection, io_threads) = Connection::stdio();
    run_connection(connection)?;
    io_threads.join()?;
    Ok(())
}

pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..ServerCapabilities::default()
    }
}

pub fn run_connection(connection: Connection) -> Result<()> {
    let (id, init_params) = connection.initialize_start()?;
    let init_params: lsp_types::InitializeParams =
        serde_json::from_value(init_params).context("invalid initialize params")?;
    let workspace_root = root_from_workspace_folders(init_params.workspace_folders.as_deref())
        .or_else(|| root_from_root_uri(&init_params))
        .and_then(|path| path.canonicalize().ok());
    let config_path = workspace_root.as_deref().and_then(resolve_config_path);
    let analyzer_config = load_analyzer_config(config_path.as_deref())?;
    connection.initialize_finish(id, serde_json::to_value(server_capabilities())?)?;

    let mut state = LspState {
        workspace_root,
        config_path,
        analyzer_config,
        ..LspState::default()
    };
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                handle_request(&connection, &state, request)?;
            }
            Message::Notification(notification) => {
                handle_notification(&connection, &mut state, notification)?;
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn handle_request(connection: &Connection, state: &LspState, request: Request) -> Result<()> {
    if request.method == CodeActionRequest::METHOD {
        let params: lsp_types::CodeActionParams =
            serde_json::from_value(request.params).context("invalid code action params")?;
        let actions = state
            .documents
            .get(&params.text_document.uri)
            .map(|document| {
                code_actions(
                    params.text_document.uri.clone(),
                    &document.text,
                    &document.analysis,
                    &document.findings,
                    params.range,
                )
            })
            .transpose()?
            .unwrap_or_default();
        let response: Option<CodeActionResponse> = Some(actions);
        connection.sender.send(Message::Response(Response {
            id: request.id,
            result: Some(serde_json::to_value(response)?),
            error: None,
        }))?;
    } else {
        connection.sender.send(Message::Response(Response {
            id: request.id,
            result: None,
            error: Some(lsp_server::ResponseError {
                code: lsp_server::ErrorCode::MethodNotFound as i32,
                message: format!("method not found: {}", request.method),
                data: None,
            }),
        }))?;
    }
    Ok(())
}

fn handle_notification(
    connection: &Connection,
    state: &mut LspState,
    notification: Notification,
) -> Result<()> {
    match notification.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let params: DidOpenTextDocumentParams =
                serde_json::from_value(notification.params).context("invalid didOpen params")?;
            handle_did_open(connection, state, params)?;
        }
        DidChangeTextDocument::METHOD => {
            let params: DidChangeTextDocumentParams =
                serde_json::from_value(notification.params).context("invalid didChange params")?;
            handle_did_change(connection, state, params)?;
        }
        DidSaveTextDocument::METHOD => {
            let params: DidSaveTextDocumentParams =
                serde_json::from_value(notification.params).context("invalid didSave params")?;
            handle_did_save(connection, state, params)?;
        }
        DidCloseTextDocument::METHOD => {
            let params: DidCloseTextDocumentParams =
                serde_json::from_value(notification.params).context("invalid didClose params")?;
            handle_did_close(connection, state, params)?;
        }
        _ => {}
    }
    Ok(())
}

fn handle_did_open(
    connection: &Connection,
    state: &mut LspState,
    params: DidOpenTextDocumentParams,
) -> Result<()> {
    let uri = params.text_document.uri;
    let path = uri_to_path(&uri);
    state.open(
        uri.clone(),
        path,
        params.text_document.text,
        Some(params.text_document.version),
    )?;
    publish_document_diagnostics(connection, &uri, state)
}

fn handle_did_change(
    connection: &Connection,
    state: &mut LspState,
    params: DidChangeTextDocumentParams,
) -> Result<()> {
    let uri = params.text_document.uri;
    state.change(
        uri.clone(),
        params.content_changes,
        Some(params.text_document.version),
    )?;
    publish_document_diagnostics(connection, &uri, state)
}

fn handle_did_save(
    connection: &Connection,
    state: &mut LspState,
    params: DidSaveTextDocumentParams,
) -> Result<()> {
    let uri = params.text_document.uri;
    if state.config_path.as_deref() == Some(&uri_to_path(&uri))
        || is_config_uri(&uri, &state.workspace_root)
    {
        state.refresh_config_for_path(&uri_to_path(&uri))?;
    }
    state.save(&uri, params.text)?;
    publish_document_diagnostics(connection, &uri, state)
}

fn handle_did_close(
    connection: &Connection,
    state: &mut LspState,
    params: DidCloseTextDocumentParams,
) -> Result<()> {
    state.close(&params.text_document.uri);
    publish_diagnostics(connection, params.text_document.uri, Vec::new(), None)
}

fn publish_document_diagnostics(
    connection: &Connection,
    uri: &Uri,
    state: &LspState,
) -> Result<()> {
    let Some(document) = state.documents.get(uri) else {
        return Ok(());
    };
    let diagnostics =
        diagnostics_for_analysis(&document.text, &document.analysis, &document.findings);
    publish_diagnostics(connection, uri.clone(), diagnostics, document.version)
}

fn publish_diagnostics(
    connection: &Connection,
    uri: Uri,
    diagnostics: Vec<Diagnostic>,
    version: Option<i32>,
) -> Result<()> {
    let params = PublishDiagnosticsParams::new(uri, diagnostics, version);
    connection.sender.send(Message::Notification(Notification {
        method: PublishDiagnostics::METHOD.to_string(),
        params: serde_json::to_value(params)?,
    }))?;
    Ok(())
}

struct AnalyzedDocument {
    analysis: Arc<ProjectAnalysis>,
    presentation: SnapshotPresentationMap,
    logical_path: PathBuf,
    report: FileReport,
}

fn analyze_document(
    path: &Path,
    text: &str,
    config: &AnalyzerConfig,
    previous: Option<&Arc<ProjectAnalysis>>,
) -> Result<AnalyzedDocument> {
    let built = ProjectSnapshotPlanner::build_single_source_overlay(
        std::env::current_dir().context("resolve LSP invocation base")?,
        path,
        text.as_bytes().to_vec(),
    )?;
    let logical_path = built
        .snapshot
        .entries()
        .next()
        .ok_or_else(|| anyhow::anyhow!("LSP overlay snapshot is empty"))?
        .path()
        .to_path_buf();
    let analysis = match previous {
        Some(previous) => previous.successor(built.snapshot)?.into_current(),
        None => ProjectAnalysis::build(built.snapshot)?,
    };
    let projection = scan_analysis_with_presentation(
        Arc::clone(&analysis),
        &built.presentation,
        document_analyzer_config(config),
    )?;
    let report = projection
        .reports
        .into_iter()
        .find(|report| report.path == path)
        .ok_or_else(|| anyhow::anyhow!("analyzer returned no LSP report for {}", path.display()))?;
    Ok(AnalyzedDocument {
        analysis,
        presentation: built.presentation,
        logical_path,
        report,
    })
}

fn document_analyzer_config(config: &AnalyzerConfig) -> AnalyzerConfig {
    let mut config = config.clone();
    config.boundary.enabled = false;
    config
}

#[cfg(test)]
fn analyze_text_with_config(path: &Path, text: &str, config: &AnalyzerConfig) -> FileReport {
    analyze_document(path, text, config, None)
        .map(|analyzed| analyzed.report)
        .unwrap_or_else(|error| FileReport {
            path: path.to_path_buf(),
            lang: deslop_core::Lang::Generic,
            analysis: AnalysisProvenance::failed(vec![AnalysisDiagnostic {
                code: "lsp-analysis-failed".to_string(),
                message: error.to_string(),
                span: None,
            }]),
            findings: Vec::new(),
        })
}

fn root_from_workspace_folders(folders: Option<&[lsp_types::WorkspaceFolder]>) -> Option<PathBuf> {
    folders
        .and_then(|folders| folders.first())
        .map(|folder| uri_to_path(&folder.uri))
}

#[allow(deprecated)]
fn root_from_root_uri(params: &lsp_types::InitializeParams) -> Option<PathBuf> {
    params.root_uri.as_ref().map(uri_to_path)
}

fn resolve_config_path(root: &Path) -> Option<PathBuf> {
    let candidate = root.join("deslop.toml");
    candidate.exists().then_some(candidate)
}

fn is_config_uri(uri: &Uri, workspace_root: &Option<PathBuf>) -> bool {
    uri_to_path(uri)
        .file_name()
        .is_some_and(|name| name == "deslop.toml")
        && workspace_root
            .as_ref()
            .is_none_or(|root| uri_to_path(uri).starts_with(root))
}

fn load_analyzer_config(path: Option<&Path>) -> Result<AnalyzerConfig> {
    let Some(path) = path else {
        return Ok(AnalyzerConfig::default());
    };
    if !path.exists() {
        return Ok(AnalyzerConfig::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read analyzer config {}", path.display()))?;
    let config: LspConfig = toml::from_str(&text)
        .with_context(|| format!("failed to parse analyzer config {}", path.display()))?;
    let analyzer = config.analyzer.unwrap_or_default();
    let defaults = AnalyzerConfig::default();
    let mut suppression = Suppression::builder();
    suppression.add_section(
        analyzer.disabled_rules.as_deref().unwrap_or_default(),
        analyzer.ignore_paths.as_deref().unwrap_or_default(),
        analyzer.rules.iter().flatten().map(|(rule, rule_config)| {
            (
                rule.as_str(),
                RuleSuppression {
                    enabled: rule_config.enabled,
                    ignore_paths: rule_config.ignore_paths.as_deref().unwrap_or_default(),
                },
            )
        }),
    );

    let suppression = suppression.build()?;

    Ok(AnalyzerConfig {
        min_duplication_tokens: analyzer
            .min_duplication_tokens
            .unwrap_or(defaults.min_duplication_tokens),
        long_method_nloc: analyzer
            .long_method_nloc
            .unwrap_or(defaults.long_method_nloc),
        min_meaningful_tokens: analyzer
            .min_meaningful_tokens
            .unwrap_or(defaults.min_meaningful_tokens),
        rust: lsp_lang_threshold(analyzer.rust.as_ref()),
        clojure: lsp_lang_threshold(analyzer.clojure.as_ref()),
        julia: lsp_lang_threshold(analyzer.julia.as_ref()),
        python: lsp_lang_threshold(analyzer.python.as_ref()),
        javascript: lsp_lang_threshold(analyzer.javascript.as_ref()),
        typescript: lsp_lang_threshold(analyzer.typescript.as_ref()),
        generic: lsp_lang_threshold(analyzer.generic.as_ref()),
        suppression,
        ..defaults
    })
}

fn lsp_lang_threshold(configured: Option<&LspAnalyzerLangConfig>) -> AnalyzerLangConfig {
    AnalyzerLangConfig {
        long_method_nloc: configured.and_then(|lang| lang.long_method_nloc),
    }
}

pub fn diagnostics_for_findings(text: &str, findings: &[Finding]) -> Vec<Diagnostic> {
    findings
        .iter()
        .map(|finding| finding_to_diagnostic(text, finding))
        .collect()
}

pub fn diagnostics_for_analysis(
    text: &str,
    analysis: &AnalysisProvenance,
    findings: &[Finding],
) -> Vec<Diagnostic> {
    let mut diagnostics = analysis
        .diagnostics
        .iter()
        .map(|diagnostic| analysis_diagnostic(text, diagnostic))
        .collect::<Vec<_>>();
    diagnostics.extend(diagnostics_for_findings(text, findings));
    diagnostics
}

fn analysis_diagnostic(text: &str, diagnostic: &AnalysisDiagnostic) -> Diagnostic {
    let range = diagnostic.span.map_or_else(
        || Range::new(Position::new(0, 0), Position::new(0, 0)),
        |span| {
            Range::new(
                byte_offset_position_utf16(text, span.start_byte, span.start_line),
                byte_offset_position_utf16(text, span.end_byte, span.end_line),
            )
        },
    );
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(diagnostic.code.clone())),
        source: Some("deslop".to_string()),
        message: diagnostic.message.clone(),
        ..Diagnostic::default()
    }
}

pub fn finding_to_diagnostic(text: &str, finding: &Finding) -> Diagnostic {
    Diagnostic {
        range: finding_range(text, finding),
        severity: Some(severity_to_diagnostic(finding.severity)),
        code: Some(NumberOrString::String(finding.rule.clone())),
        source: Some("deslop".to_string()),
        message: finding.message.clone(),
        ..Diagnostic::default()
    }
}

pub fn code_actions(
    uri: Uri,
    text: &str,
    analysis: &AnalysisProvenance,
    findings: &[Finding],
    requested_range: Range,
) -> Result<Vec<CodeActionOrCommand>> {
    if !analysis.permits_rewrites() {
        return Ok(Vec::new());
    }
    let mut actions = Vec::new();
    actions.extend(fix_all_action(uri.clone(), text, findings)?);
    for finding in findings {
        if !is_code_action_fixable(finding)
            || !ranges_overlap(&finding_range(text, finding), &requested_range)
        {
            continue;
        }
        let fixed = apply_findings_to_text(text, std::slice::from_ref(finding))?;
        if fixed == text {
            continue;
        }
        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "deslop: apply safe fix".to_string(),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![finding_to_diagnostic(text, finding)]),
            edit: Some(WorkspaceEdit {
                document_changes: Some(DocumentChanges::Edits(vec![whole_document_edit(
                    uri.clone(),
                    text,
                    fixed,
                )])),
                ..WorkspaceEdit::default()
            }),
            is_preferred: Some(true),
            ..CodeAction::default()
        }));
    }
    Ok(actions)
}

fn fix_all_action(
    uri: Uri,
    text: &str,
    findings: &[Finding],
) -> Result<Option<CodeActionOrCommand>> {
    let fixable = findings
        .iter()
        .filter(|finding| is_code_action_fixable(finding))
        .cloned()
        .collect::<Vec<_>>();
    if fixable.is_empty() {
        return Ok(None);
    }
    let fixed = apply_findings_to_text(text, &fixable)?;
    if fixed == text {
        return Ok(None);
    }
    Ok(Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: "deslop: fix all safe findings in file".to_string(),
        kind: Some(CodeActionKind::SOURCE_FIX_ALL),
        edit: Some(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![whole_document_edit(
                uri, text, fixed,
            )])),
            ..WorkspaceEdit::default()
        }),
        is_preferred: Some(true),
        ..CodeAction::default()
    })))
}

fn whole_document_edit(uri: Uri, text: &str, fixed: String) -> TextDocumentEdit {
    TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
        edits: vec![OneOf::Left(TextEdit::new(full_document_range(text), fixed))],
    }
}

fn is_code_action_fixable(finding: &Finding) -> bool {
    matches!(
        finding.safety,
        SafetyClass::SafeAuto | SafetyClass::AnalyzerConfirmed
    ) && finding.edit.is_some()
}

fn severity_to_diagnostic(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Major => DiagnosticSeverity::ERROR,
        Severity::Minor => DiagnosticSeverity::WARNING,
        Severity::Info => DiagnosticSeverity::HINT,
    }
}

fn finding_range(text: &str, finding: &Finding) -> Range {
    Range::new(
        byte_offset_position_utf16(text, finding.span.start_byte, finding.span.start_line),
        byte_offset_position_utf16(text, finding.span.end_byte, finding.span.end_line),
    )
}

fn byte_offset_position_utf16(text: &str, byte_offset: usize, fallback_line: usize) -> Position {
    let line_start = line_start_byte(text, fallback_line).unwrap_or(0);
    let bounded_offset = byte_offset.min(text.len());
    let mut character = 0_u32;
    for (idx, ch) in text[line_start..].char_indices() {
        let absolute = line_start + idx;
        if absolute >= bounded_offset || ch == '\n' {
            break;
        }
        character += ch.len_utf16() as u32;
    }
    Position::new(fallback_line.saturating_sub(1) as u32, character)
}

fn line_start_byte(text: &str, one_based_line: usize) -> Option<usize> {
    if one_based_line == 0 {
        return None;
    }
    if one_based_line == 1 {
        return Some(0);
    }
    let mut line = 1;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            line += 1;
            if line == one_based_line {
                return Some(idx + ch.len_utf8());
            }
        }
    }
    None
}

fn full_document_range(text: &str) -> Range {
    let mut line = 0_u32;
    let mut character = 0_u32;
    for (idx, part) in text.split('\n').enumerate() {
        line = idx as u32;
        character = part.encode_utf16().count() as u32;
    }
    Range::new(Position::new(0, 0), Position::new(line, character))
}

fn apply_text_document_changes(
    original: &str,
    changes: Vec<TextDocumentContentChangeEvent>,
) -> Result<String> {
    let mut text = original.to_string();
    for change in changes {
        if let Some(range) = change.range {
            let start = position_to_byte_offset(&text, range.start)?;
            let end = position_to_byte_offset(&text, range.end)?;
            if start > end {
                bail!("invalid LSP change range");
            }
            text.replace_range(start..end, &change.text);
        } else {
            text = change.text;
        }
    }
    Ok(text)
}

fn position_to_byte_offset(text: &str, position: Position) -> Result<usize> {
    let line_start = line_start_byte(text, position.line as usize + 1)
        .with_context(|| format!("line {} is out of range", position.line))?;
    let mut utf16 = 0_u32;
    for (idx, ch) in text[line_start..].char_indices() {
        if ch == '\n' {
            break;
        }
        if utf16 == position.character {
            return Ok(line_start + idx);
        }
        utf16 += ch.len_utf16() as u32;
        if utf16 > position.character {
            bail!("position falls inside a UTF-16 surrogate pair");
        }
    }
    if utf16 == position.character {
        Ok(text[line_start..]
            .find('\n')
            .map_or(text.len(), |idx| line_start + idx))
    } else {
        bail!("character {} is out of range", position.character)
    }
}

fn ranges_overlap(left: &Range, right: &Range) -> bool {
    position_less_or_equal(left.start, right.end) && position_less_or_equal(right.start, left.end)
}

fn position_less_or_equal(left: Position, right: Position) -> bool {
    left.line < right.line || (left.line == right.line && left.character <= right.character)
}

fn uri_to_path(uri: &Uri) -> PathBuf {
    let value = uri.as_str();
    let path = value
        .strip_prefix("file://")
        .unwrap_or(value)
        .trim_start_matches("localhost");
    PathBuf::from(percent_decode(path))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%'
            && idx + 2 < bytes.len()
            && let Ok(hex) = u8::from_str_radix(&value[idx + 1..idx + 3], 16)
        {
            out.push(hex);
            idx += 3;
            continue;
        }
        out.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_analyzer::scan_source;
    use deslop_core::{DetectedBy, Span};
    use deslop_parse::SourceFile;
    use serde_json::json;
    use std::str::FromStr;
    use std::thread;
    use std::time::Duration;

    fn uri() -> Uri {
        Uri::from_str("file:///sample.clj").expect("uri")
    }

    fn sample_source(text: &str) -> SourceFile {
        SourceFile::new(PathBuf::from("sample.clj"), text.to_string())
    }

    fn sample_findings(text: &str) -> Vec<Finding> {
        scan_source(&sample_source(text)).findings
    }

    fn requested_range() -> Range {
        Range::new(Position::new(0, 0), Position::new(10, 0))
    }

    #[test]
    fn maps_finding_to_diagnostic() {
        let text = "(not (= a b))\n";
        let findings = sample_findings(text);
        let diagnostic = diagnostics_for_findings(text, &findings)
            .into_iter()
            .find(|diagnostic| {
                diagnostic.code == Some(NumberOrString::String("reimpl-not=".to_string()))
            })
            .expect("diagnostic");

        assert_eq!(diagnostic.range.start, Position::new(0, 0));
        assert_eq!(diagnostic.range.end.line, 0);
        assert!(diagnostic.range.end.character > 0);
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(diagnostic.source.as_deref(), Some("deslop"));
        assert_eq!(
            diagnostic.code,
            Some(NumberOrString::String("reimpl-not=".to_string()))
        );
        assert!(diagnostic.message.contains("not="));
    }

    #[test]
    fn diagnostic_range_uses_precise_utf16_columns_for_non_ascii() {
        let text = "é𝄞(not (= a b))\n";
        let prefix = "é𝄞";
        let matched = "(not (= a b))";
        let finding = Finding {
            path: PathBuf::from("sample.clj"),
            span: Span::new(1, 1, prefix.len(), prefix.len() + matched.len()),
            rule: "reimpl-not=".to_string(),
            severity: Severity::Minor,
            safety: SafetyClass::SafeAuto,
            detected_by: DetectedBy::Idiom,
            message: "test".to_string(),
            suggestion: "test".to_string(),
            precondition: None,
            edit: None,
            fingerprint: "test".to_string(),
        };

        let diagnostic = finding_to_diagnostic(text, &finding);

        assert_eq!(diagnostic.range.start, Position::new(0, 3));
        assert_eq!(
            diagnostic.range.end,
            Position::new(0, 3 + matched.encode_utf16().count() as u32)
        );
    }

    #[test]
    fn tsx_document_analysis_uses_the_path_selected_grammar() {
        let text = "function View(value: string): JSX.Element {\n  // deslop:ignore-next-line js-var-declaration\n  var copy: JSX.Element = <span>{value}</span>;\n  return copy;\n}\n";
        let report =
            analyze_text_with_config(Path::new("sample.tsx"), text, &AnalyzerConfig::default());

        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.rule == "js-var-declaration")
        );
    }

    #[test]
    fn malformed_document_publishes_parse_diagnostic_and_no_code_actions() -> Result<()> {
        let text = include_str!("../../../tests/fixtures/typescript/malformed.ts");
        let report =
            analyze_text_with_config(Path::new("malformed.ts"), text, &AnalyzerConfig::default());

        let diagnostics = diagnostics_for_analysis(text, &report.analysis, &report.findings);
        assert_eq!(report.analysis.status, deslop_core::AnalysisStatus::Partial);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == Some(NumberOrString::String("tree-sitter-error".to_string()))
        }));
        assert!(
            code_actions(
                uri(),
                text,
                &report.analysis,
                &report.findings,
                requested_range(),
            )?
            .is_empty()
        );
        Ok(())
    }

    #[test]
    fn code_actions_only_offer_safe_fixable_findings() -> Result<()> {
        let text = "(not (= a b))\n(= (count xs) 0)\n";
        let findings = sample_findings(text);
        let safe = findings
            .iter()
            .find(|finding| finding.rule == "reimpl-not=")
            .expect("safe finding")
            .clone();
        let llm_only = Finding {
            safety: SafetyClass::LlmOnly,
            edit: None,
            rule: "long-method".to_string(),
            message: "long method".to_string(),
            span: safe.span,
            ..safe.clone()
        };
        let never_auto = Finding {
            safety: SafetyClass::NeverAuto,
            rule: "missing-reference".to_string(),
            message: "unresolved reference".to_string(),
            ..safe.clone()
        };

        let safe_actions = code_actions(
            uri(),
            text,
            &AnalysisProvenance::complete(),
            std::slice::from_ref(&safe),
            requested_range(),
        )?;
        assert_eq!(safe_actions.len(), 2);
        let action = first_action_with_kind(&safe_actions, CodeActionKind::QUICKFIX);
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        assert!(action.edit.is_some());

        let risky_actions = code_actions(
            uri(),
            text,
            &AnalysisProvenance::complete(),
            &[llm_only],
            requested_range(),
        )?;
        assert!(risky_actions.is_empty());

        let report_only_actions = code_actions(
            uri(),
            text,
            &AnalysisProvenance::complete(),
            &[never_auto],
            requested_range(),
        )?;
        assert!(report_only_actions.is_empty());
        Ok(())
    }

    #[test]
    fn code_actions_include_fix_all_for_safe_findings_only() -> Result<()> {
        let text = "(not (= a b))\n(not (nil? x))\n(= (count xs) 0)\n";
        let findings = sample_findings(text);
        let actions = code_actions(
            uri(),
            text,
            &AnalysisProvenance::complete(),
            &findings,
            requested_range(),
        )?;

        let fix_all = first_action_with_kind(&actions, CodeActionKind::SOURCE_FIX_ALL);
        assert_eq!(action_kind_count(&actions, CodeActionKind::QUICKFIX), 2);

        let fixed = first_replacement_text(fix_all);
        assert!(fixed.contains("(not= a b)"));
        assert!(fixed.contains("(some? x)"));
        assert!(fixed.contains("(= (count xs) 0)"));

        let risky_text = "(= (count xs) 0)\n";
        let risky_actions = code_actions(
            uri(),
            risky_text,
            &AnalysisProvenance::complete(),
            &sample_findings(risky_text),
            requested_range(),
        )?;
        assert_eq!(
            action_kind_count(&risky_actions, CodeActionKind::SOURCE_FIX_ALL),
            0
        );
        Ok(())
    }

    #[test]
    fn incremental_change_applies_utf16_ranges_with_non_ascii() -> Result<()> {
        let text = "é𝄞abc\n";
        let changed = apply_text_document_changes(
            text,
            vec![TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 3), Position::new(0, 4))),
                range_length: None,
                text: "Z".to_string(),
            }],
        )?;

        assert_eq!(changed, "é𝄞Zbc\n");
        Ok(())
    }

    #[test]
    fn document_lifecycle_owns_one_incremental_parse_per_revision() -> Result<()> {
        let uri = uri();
        let path = PathBuf::from("/sample.clj");
        let mut state = LspState::default();
        deslop_parse::reset_parse_source_invocations();

        state.open(uri.clone(), path, "(not (= a b))\n".to_string(), Some(1))?;
        let first = Arc::clone(&state.documents[&uri].project_analysis);
        let first_id = first.id().clone();
        assert_eq!(first.parse_counts().len(), 1);
        assert!(first.parse_counts().values().all(|count| {
            (
                count.requested,
                count.owners,
                count.parser_invocations,
                count.reused,
            ) == (1, 1, 1, 0)
        }));

        state.change(
            uri.clone(),
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "(not (= a c))\n".to_string(),
            }],
            Some(2),
        )?;
        let second = Arc::clone(&state.documents[&uri].project_analysis);
        assert_ne!(second.id(), &first_id);
        assert_eq!(second.parse_counts().len(), 1);
        assert!(second.parse_counts().values().all(|count| {
            (
                count.requested,
                count.owners,
                count.parser_invocations,
                count.reused,
            ) == (1, 1, 1, 0)
        }));
        assert_eq!(first.parse_counts().len(), 1);

        let second_id = second.id().clone();
        let second_counts = second.parse_counts();
        state.save(&uri, None)?;
        let saved = &state.documents[&uri].project_analysis;
        assert_eq!(saved.id(), &second_id);
        assert_eq!(saved.parse_counts(), second_counts);
        assert_eq!(deslop_parse::parse_source_invocations(), 0);
        Ok(())
    }

    #[test]
    fn document_analysis_path_has_no_legacy_parse_or_reselection() {
        let source = include_str!("lib.rs");
        let start = source
            .find("impl LspState {")
            .expect("state implementation");
        let end = source[start..]
            .find("#[cfg(test)]\nfn analyze_text_with_config")
            .map(|offset| start + offset)
            .expect("test-only compatibility helper");
        let ownership_path = &source[start..end];

        for forbidden in [
            "scan_source",
            "parse_source",
            "SourceFile::read",
            "read_to_string",
            "pack_for_path",
            "supported_pack_for_path",
            "pack_for_lang",
        ] {
            assert!(
                !ownership_path.contains(forbidden),
                "LSP document analysis reintroduced forbidden operation {forbidden}"
            );
        }
    }

    fn first_replacement_text(action: &CodeAction) -> String {
        let edit = action.edit.as_ref().expect("edit");
        let Some(DocumentChanges::Edits(edits)) = &edit.document_changes else {
            panic!("expected document changes");
        };
        let OneOf::Left(text_edit) = &edits[0].edits[0] else {
            panic!("expected text edit");
        };
        text_edit.new_text.clone()
    }

    fn first_action_with_kind(
        actions: &[CodeActionOrCommand],
        kind: CodeActionKind,
    ) -> &CodeAction {
        actions
            .iter()
            .find_map(|action| match action {
                CodeActionOrCommand::CodeAction(action) if action.kind == Some(kind.clone()) => {
                    Some(action)
                }
                _ => None,
            })
            .expect("code action")
    }

    fn action_kind_count(actions: &[CodeActionOrCommand], kind: CodeActionKind) -> usize {
        actions
            .iter()
            .filter(|action| match action {
                CodeActionOrCommand::CodeAction(action) => action.kind == Some(kind.clone()),
                _ => false,
            })
            .count()
    }

    #[test]
    fn json_rpc_loop_publishes_diagnostics_and_code_actions() {
        let (server, client) = Connection::memory();
        let server_thread = thread::spawn(move || run_connection(server).expect("server"));
        let uri = uri();
        let text = "(not (= a b))\n";

        initialize(&client);
        open_document(&client, &uri, text);
        let diagnostics = assert_reimpl_not_diagnostics(&client, &uri);
        let actions = request_code_actions(&client, &uri, diagnostics.diagnostics);
        assert!(action_kind_count(&actions, CodeActionKind::QUICKFIX) > 0);
        assert!(action_kind_count(&actions, CodeActionKind::SOURCE_FIX_ALL) > 0);

        shutdown(&client, server_thread);
    }

    fn initialize(connection: &Connection) {
        send_request(
            connection,
            1,
            "initialize",
            json!({ "capabilities": {} }),
            "send initialize",
        );
        let initialize = recv_response(connection);
        assert!(initialize.error.is_none(), "{initialize:#?}");
        assert!(initialize.result.is_some());
        send_notification(connection, "initialized", json!({}), "send initialized");
    }

    fn open_document(connection: &Connection, uri: &Uri, text: &str) {
        send_notification(
            connection,
            DidOpenTextDocument::METHOD,
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "clojure",
                    "version": 1,
                    "text": text
                }
            }),
            "send didOpen",
        );
    }

    fn assert_reimpl_not_diagnostics(
        connection: &Connection,
        uri: &Uri,
    ) -> PublishDiagnosticsParams {
        let diagnostics = recv_publish_diagnostics(connection);
        assert_eq!(&diagnostics.uri, uri);
        assert!(
            diagnostics
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code
                    == Some(NumberOrString::String("reimpl-not=".to_string()))),
            "{diagnostics:#?}"
        );
        diagnostics
    }

    fn request_code_actions(
        connection: &Connection,
        uri: &Uri,
        diagnostics: Vec<Diagnostic>,
    ) -> Vec<CodeActionOrCommand> {
        send_request(
            connection,
            2,
            CodeActionRequest::METHOD,
            json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 20 }
                },
                "context": { "diagnostics": diagnostics }
            }),
            "send codeAction",
        );
        let response = recv_response(connection);
        assert!(response.error.is_none(), "{response:#?}");
        serde_json::from_value(response.result.expect("codeAction result")).expect("actions")
    }

    fn shutdown(connection: &Connection, server_thread: thread::JoinHandle<()>) {
        send_request(
            connection,
            3,
            "shutdown",
            serde_json::Value::Null,
            "send shutdown",
        );
        let shutdown = recv_response(connection);
        assert!(shutdown.error.is_none(), "{shutdown:#?}");
        send_notification(connection, "exit", serde_json::Value::Null, "send exit");
        server_thread.join().expect("join server");
    }

    fn send_request(
        connection: &Connection,
        id: i32,
        method: &str,
        params: serde_json::Value,
        context: &str,
    ) {
        connection
            .sender
            .send(Message::Request(Request {
                id: id.into(),
                method: method.to_string(),
                params,
            }))
            .expect(context);
    }

    fn send_notification(
        connection: &Connection,
        method: &str,
        params: serde_json::Value,
        context: &str,
    ) {
        connection
            .sender
            .send(Message::Notification(Notification {
                method: method.to_string(),
                params,
            }))
            .expect(context);
    }

    fn recv_response(connection: &Connection) -> Response {
        match connection
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("receive response")
        {
            Message::Response(response) => response,
            other => panic!("expected response, got {other:?}"),
        }
    }

    fn recv_publish_diagnostics(connection: &Connection) -> PublishDiagnosticsParams {
        match connection
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("receive diagnostics")
        {
            Message::Notification(notification)
                if notification.method == PublishDiagnostics::METHOD =>
            {
                serde_json::from_value(notification.params).expect("diagnostic params")
            }
            other => panic!("expected publishDiagnostics, got {other:?}"),
        }
    }
}
