use std::path::{Path, PathBuf};

use deslop_core::{Finding, SafetyClass, Severity, Span, fingerprint};
use deslop_parse::SourceFile;
use serde::{Deserialize, Serialize};

macro_rules! protocol_struct {
    ($vis:vis struct $name:ident { $($field:ident: $type:ty),+ $(,)? }) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        $vis struct $name {
            $(pub $field: $type),+
        }
    };
}

protocol_struct! {
pub struct Region {
    start_line: usize,
    end_line: usize,
    text: String,
}
}

protocol_struct! {
pub struct WorkOrderFinding {
    rule: String,
    severity: deslop_core::Severity,
    safety: SafetyClass,
    message: String,
    precondition: Option<String>,
}
}

protocol_struct! {
pub struct Contract {
    must_parse: bool,
    no_new_public_defs: bool,
    keep_error_handling: bool,
    max_growth_ratio: f32,
    check_cmd: Option<String>,
}
}

impl Default for Contract {
    fn default() -> Self {
        Self {
            must_parse: true,
            no_new_public_defs: true,
            keep_error_handling: true,
            max_growth_ratio: 1.0,
            check_cmd: None,
        }
    }
}

protocol_struct! {
pub struct WorkOrder {
    schema: String,
    kind: WorkOrderKind,
    id: String,
    path: PathBuf,
    region: Region,
    findings: Vec<WorkOrderFinding>,
    instruction: String,
    contract: Contract,
}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkOrderKind {
    RewriteRegion,
    NeedsCharacterizationTest,
}

protocol_struct! {
pub struct Patch {
    schema: String,
    workorder_id: String,
    region_fingerprint: String,
    replacement: String,
    by: String,
}
}

protocol_struct! {
pub struct CharacterizationTest {
    schema: String,
    workorder_id: String,
    region_fingerprint: String,
    test_path: PathBuf,
    test_text: String,
    by: String,
}
}

pub fn work_orders_for_source(source: &SourceFile, findings: &[Finding]) -> Vec<WorkOrder> {
    findings
        .iter()
        .filter(|finding| finding.safety != SafetyClass::SafeAuto)
        .map(|finding| work_order_for_finding(source, finding))
        .collect()
}

pub fn region_fingerprint(path: &Path, region: &Region) -> String {
    let start_byte = byte_offset_for_line(&region.text, 1);
    let end_byte = region.text.len();
    let span = Span::new(region.start_line, region.end_line, start_byte, end_byte);
    fingerprint(path, "region", span, &region.text)
}

pub fn workorder_region_fingerprint(work_order: &WorkOrder) -> String {
    region_fingerprint(&work_order.path, &work_order.region)
}

pub fn workorder_id_for_region(path: &Path, region: &Region) -> String {
    format!("wo_{}", region_fingerprint(path, region))
}

fn work_order_for_finding(source: &SourceFile, finding: &Finding) -> WorkOrder {
    let region_span =
        source.enclosing_region_for_span(finding.span.start_line, finding.span.end_line);
    let region = Region {
        start_line: region_span.start_line,
        end_line: region_span.end_line,
        text: source.region_text(region_span.start_line, region_span.end_line),
    };
    let id = workorder_id_for_region(&finding.path, &region);
    WorkOrder {
        schema: "deslop.workorder/1".to_string(),
        kind: WorkOrderKind::RewriteRegion,
        id,
        path: finding.path.to_path_buf(),
        region,
        findings: vec![WorkOrderFinding {
            rule: finding.rule.to_owned(),
            severity: finding.severity,
            safety: finding.safety,
            message: finding.message.to_owned(),
            precondition: finding.precondition.to_owned(),
        }],
        instruction: "Rewrite the region to remove the flagged bloat without changing behavior or the public API. Preserve language and indentation.".to_string(),
        contract: Contract::default(),
    }
}

pub fn characterization_work_order_for(work_order: &WorkOrder) -> WorkOrder {
    WorkOrder {
        schema: "deslop.workorder/1".to_string(),
        kind: WorkOrderKind::NeedsCharacterizationTest,
        id: work_order.id.to_owned(),
        path: work_order.path.to_path_buf(),
        region: work_order.region.clone(),
        findings: vec![WorkOrderFinding {
            rule: "needs-characterization-test".to_string(),
            severity: Severity::Major,
            safety: SafetyClass::LlmOnly,
            message: "region has a weak test oracle; generate a characterization test before removal".to_string(),
            precondition: None,
        }],
        instruction: "Write a test that pins the current observable behavior of this exact region. Do not change production behavior. Return deslop.characterization-test/1 JSONL with test_path and test_text; the test must compile and pass against the current unmodified code.".to_string(),
        contract: Contract {
            must_parse: true,
            no_new_public_defs: false,
            keep_error_handling: true,
            max_growth_ratio: 1.0,
            check_cmd: work_order.contract.check_cmd.to_owned(),
        },
    }
}

fn byte_offset_for_line(text: &str, one_based_line: usize) -> usize {
    if one_based_line <= 1 {
        return 0;
    }
    let mut current_line = 1;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            current_line += 1;
            if current_line == one_based_line {
                return idx + 1;
            }
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_core::{DetectedBy, SafetyClass, Severity, Span};

    #[test]
    fn workorder_schema_matches_spec_surface() {
        let source = SourceFile::new(PathBuf::from("sample.clj"), "(= (count xs) 0)\n".into());
        let finding = Finding {
            path: source.path.to_path_buf(),
            span: Span::new(1, 1, 0, source.text.len()),
            rule: "reimpl-empty?".to_string(),
            severity: Severity::Minor,
            safety: SafetyClass::SafeWithPrecondition,
            detected_by: DetectedBy::Idiom,
            message: "message".to_string(),
            suggestion: "suggestion".to_string(),
            precondition: Some("finite".to_string()),
            edit: None,
            fingerprint: "finding".to_string(),
        };
        let work_order = work_orders_for_source(&source, &[finding]).remove(0);
        let value = serde_json::to_value(&work_order).expect("json");
        assert!(value.get("schema").is_some());
        assert_eq!(value["kind"], "rewrite-region");
        assert!(value.get("id").is_some());
        assert!(value.get("path").is_some());
        assert!(value.get("region").is_some());
        assert!(value.get("findings").is_some());
        assert!(value.get("instruction").is_some());
        assert!(value.get("contract").is_some());
        assert!(value.get("region_fingerprint").is_none());
    }
}
