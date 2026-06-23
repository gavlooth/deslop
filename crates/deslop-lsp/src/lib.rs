use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use deslop_analyzer::scan_source;
use deslop_core::{Finding, SafetyClass, Severity};
use deslop_fix::apply_findings_to_text;
use deslop_parse::SourceFile;
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
    PublishDiagnosticsParams, Range, ServerCapabilities, TextDocumentEdit,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri, WorkspaceEdit,
};

#[derive(Debug, Clone)]
struct DocumentState {
    text: String,
    findings: Vec<Finding>,
    version: Option<i32>,
}

#[derive(Debug, Default)]
struct LspState {
    documents: BTreeMap<Uri, DocumentState>,
}

impl LspState {
    fn open(&mut self, uri: Uri, text: String, version: Option<i32>) {
        let findings = analyze_text(&uri, &text);
        self.documents.insert(
            uri,
            DocumentState {
                text,
                findings,
                version,
            },
        );
    }

    fn change(&mut self, uri: Uri, text: String, version: Option<i32>) {
        self.open(uri, text, version);
    }

    fn save(&mut self, uri: &Uri, text: Option<String>) {
        if let Some(text) = text {
            self.open(uri.clone(), text, None);
        } else if let Some(document) = self.documents.get_mut(uri) {
            document.findings = analyze_text(uri, &document.text);
        }
    }

    fn close(&mut self, uri: &Uri) {
        self.documents.remove(uri);
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
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        ..ServerCapabilities::default()
    }
}

pub fn run_connection(connection: Connection) -> Result<()> {
    let (id, _params) = connection.initialize_start()?;
    connection.initialize_finish(id, serde_json::to_value(server_capabilities())?)?;

    let mut state = LspState::default();
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
            let uri = params.text_document.uri;
            state.open(
                uri.clone(),
                params.text_document.text,
                Some(params.text_document.version),
            );
            publish_document_diagnostics(connection, &uri, state)?;
        }
        DidChangeTextDocument::METHOD => {
            let params: DidChangeTextDocumentParams =
                serde_json::from_value(notification.params).context("invalid didChange params")?;
            let Some(change) = params.content_changes.into_iter().last() else {
                return Ok(());
            };
            let uri = params.text_document.uri;
            state.change(uri.clone(), change.text, Some(params.text_document.version));
            publish_document_diagnostics(connection, &uri, state)?;
        }
        DidSaveTextDocument::METHOD => {
            let params: DidSaveTextDocumentParams =
                serde_json::from_value(notification.params).context("invalid didSave params")?;
            let uri = params.text_document.uri;
            state.save(&uri, params.text);
            publish_document_diagnostics(connection, &uri, state)?;
        }
        DidCloseTextDocument::METHOD => {
            let params: DidCloseTextDocumentParams =
                serde_json::from_value(notification.params).context("invalid didClose params")?;
            state.close(&params.text_document.uri);
            publish_diagnostics(connection, params.text_document.uri, Vec::new(), None)?;
        }
        _ => {}
    }
    Ok(())
}

fn publish_document_diagnostics(
    connection: &Connection,
    uri: &Uri,
    state: &LspState,
) -> Result<()> {
    let Some(document) = state.documents.get(uri) else {
        return Ok(());
    };
    let diagnostics = diagnostics_for_findings(&document.text, &document.findings);
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

fn analyze_text(uri: &Uri, text: &str) -> Vec<Finding> {
    let source = SourceFile::new(uri_to_path(uri), text.to_string());
    scan_source(&source).findings
}

pub fn diagnostics_for_findings(text: &str, findings: &[Finding]) -> Vec<Diagnostic> {
    findings
        .iter()
        .map(|finding| finding_to_diagnostic(text, finding))
        .collect()
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
    findings: &[Finding],
    requested_range: Range,
) -> Result<Vec<CodeActionOrCommand>> {
    let mut actions = Vec::new();
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
        let edit = TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: None,
            },
            edits: vec![OneOf::Left(TextEdit::new(full_document_range(text), fixed))],
        };
        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "deslop: apply safe fix".to_string(),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![finding_to_diagnostic(text, finding)]),
            edit: Some(WorkspaceEdit {
                document_changes: Some(DocumentChanges::Edits(vec![edit])),
                ..WorkspaceEdit::default()
            }),
            is_preferred: Some(true),
            ..CodeAction::default()
        }));
    }
    Ok(actions)
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
    let start_line = finding.span.start_line.saturating_sub(1) as u32;
    let end_line = finding.span.end_line.saturating_sub(1) as u32;
    Range::new(
        Position::new(start_line, 0),
        Position::new(end_line, line_len_utf16(text, finding.span.end_line)),
    )
}

fn line_len_utf16(text: &str, one_based_line: usize) -> u32 {
    text.lines()
        .nth(one_based_line.saturating_sub(1))
        .map(|line| line.encode_utf16().count() as u32)
        .unwrap_or(0)
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
    use std::str::FromStr;

    fn uri() -> Uri {
        Uri::from_str("file:///sample.clj").expect("uri")
    }

    #[test]
    fn maps_finding_to_diagnostic() {
        let text = "(not (= a b))\n";
        let source = SourceFile::new(PathBuf::from("sample.clj"), text.to_string());
        let report = scan_source(&source);
        let diagnostic = diagnostics_for_findings(text, &report.findings)
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
    fn code_actions_only_offer_safe_fixable_findings() -> Result<()> {
        let text = "(not (= a b))\n(= (count xs) 0)\n";
        let source = SourceFile::new(PathBuf::from("sample.clj"), text.to_string());
        let report = scan_source(&source);
        let safe = report
            .findings
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

        let requested = Range::new(Position::new(0, 0), Position::new(10, 0));
        let safe_actions = code_actions(uri(), text, &[safe], requested)?;
        assert_eq!(safe_actions.len(), 1);
        let CodeActionOrCommand::CodeAction(action) = &safe_actions[0] else {
            panic!("expected code action");
        };
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        assert!(action.edit.is_some());

        let risky_actions = code_actions(uri(), text, &[llm_only], requested)?;
        assert!(risky_actions.is_empty());
        Ok(())
    }
}
