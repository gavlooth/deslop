use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
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
    PublishDiagnosticsParams, Range, ServerCapabilities, TextDocumentContentChangeEvent,
    TextDocumentEdit, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
    WorkspaceEdit,
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
        self.open(uri, text, version);
        Ok(())
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
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
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
    state.open(
        uri.clone(),
        params.text_document.text,
        Some(params.text_document.version),
    );
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
    state.save(&uri, params.text);
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

        let safe_actions =
            code_actions(uri(), text, std::slice::from_ref(&safe), requested_range())?;
        assert_eq!(safe_actions.len(), 2);
        let action = first_action_with_kind(&safe_actions, CodeActionKind::QUICKFIX);
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
        assert!(action.edit.is_some());

        let risky_actions = code_actions(uri(), text, &[llm_only], requested_range())?;
        assert!(risky_actions.is_empty());
        Ok(())
    }

    #[test]
    fn code_actions_include_fix_all_for_safe_findings_only() -> Result<()> {
        let text = "(not (= a b))\n(not (nil? x))\n(= (count xs) 0)\n";
        let findings = sample_findings(text);
        let actions = code_actions(uri(), text, &findings, requested_range())?;

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
