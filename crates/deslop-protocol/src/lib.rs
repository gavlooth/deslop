use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use deslop_core::{
    FileReport, Finding, RevisionGuard, SafetyClass, Severity, Span, baseline_fingerprint,
    revision_guard,
};
use deslop_parse::{SourceFile, analysis_provenance_or_failed};
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
    start_byte: usize,
    end_byte: usize,
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
    region_fingerprint: String,
    revision_guard: RevisionGuard,
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
    revision_guard: RevisionGuard,
    replacement: String,
    by: String,
}
}

protocol_struct! {
pub struct CharacterizationTest {
    schema: String,
    workorder_id: String,
    revision_guard: RevisionGuard,
    test_path: PathBuf,
    test_text: String,
    by: String,
}
}

pub fn work_orders_for_report(source: &SourceFile, report: &FileReport) -> Vec<WorkOrder> {
    if source.path != report.path
        || source.lang != report.lang
        || !report.analysis.permits_rewrites()
        || !analysis_provenance_or_failed(source).permits_rewrites()
    {
        return Vec::new();
    }
    work_orders_for_source(source, &report.findings)
}

fn work_orders_for_source(source: &SourceFile, findings: &[Finding]) -> Vec<WorkOrder> {
    let mut grouped: BTreeMap<RewriteRegionKey, Vec<&Finding>> = BTreeMap::new();
    for finding in findings
        .iter()
        .filter(|finding| finding.safety != SafetyClass::SafeAuto)
    {
        let region = region_for_finding(source, finding);
        grouped
            .entry(RewriteRegionKey::new(&source.path, region))
            .or_default()
            .push(finding);
    }

    grouped
        .into_iter()
        .map(|(key, mut findings)| {
            sort_grouped_findings(&mut findings);
            work_order_for_findings(key, findings)
        })
        .collect()
}

pub fn region_fingerprint(path: &Path, region: &Region) -> String {
    baseline_fingerprint(path, "region", region_span(region), &region.text)
}

pub fn region_revision_guard(path: &Path, region: &Region) -> RevisionGuard {
    revision_guard(path, region_span(region), &region.text)
}

pub fn workorder_revision_guard(work_order: &WorkOrder) -> &RevisionGuard {
    &work_order.revision_guard
}

pub fn workorder_id_for_region(path: &Path, region: &Region) -> String {
    format!("wo2_{}", region_fingerprint(path, region))
}

pub fn validate_workorder_identity(work_order: &WorkOrder) -> Result<(), String> {
    if work_order.schema != "deslop.workorder/2" {
        return Err(format!(
            "unsupported workorder schema `{}`; regenerate as deslop.workorder/2",
            work_order.schema
        ));
    }
    let fingerprint = region_fingerprint(&work_order.path, &work_order.region);
    if work_order.region_fingerprint != fingerprint {
        return Err(
            "workorder region_fingerprint does not match its normalized region identity"
                .to_string(),
        );
    }
    let guard = region_revision_guard(&work_order.path, &work_order.region);
    if work_order.revision_guard != guard {
        return Err("workorder revision_guard does not match its exact region bytes".to_string());
    }
    let id = workorder_id_for_region(&work_order.path, &work_order.region);
    if work_order.id != id {
        return Err(format!(
            "workorder id `{}` does not match expected `{id}`",
            work_order.id
        ));
    }
    Ok(())
}

fn region_span(region: &Region) -> Span {
    Span::new(
        region.start_line,
        region.end_line,
        region.start_byte,
        region.end_byte,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RewriteRegionKey {
    path: PathBuf,
    start_line: usize,
    end_line: usize,
    start_byte: usize,
    end_byte: usize,
    text: String,
}

impl RewriteRegionKey {
    fn new(path: &Path, region: Region) -> Self {
        Self {
            path: path.to_path_buf(),
            start_line: region.start_line,
            end_line: region.end_line,
            start_byte: region.start_byte,
            end_byte: region.end_byte,
            text: region.text,
        }
    }

    fn region(&self) -> Region {
        Region {
            start_line: self.start_line,
            end_line: self.end_line,
            start_byte: self.start_byte,
            end_byte: self.end_byte,
            text: self.text.to_owned(),
        }
    }
}

fn region_for_finding(source: &SourceFile, finding: &Finding) -> Region {
    let region_span =
        source.enclosing_region_for_span(finding.span.start_line, finding.span.end_line);
    let start_byte = source.line_start_byte(region_span.start_line);
    let end_byte = source
        .line_start_byte(region_span.end_line + 1)
        .min(source.text.len());
    Region {
        start_line: region_span.start_line,
        end_line: region_span.end_line,
        start_byte,
        end_byte,
        text: source
            .text
            .get(start_byte..end_byte)
            .unwrap_or("")
            .to_string(),
    }
}

fn sort_grouped_findings(findings: &mut [&Finding]) {
    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.span.start_line.cmp(&b.span.start_line))
            .then(a.rule.cmp(&b.rule))
            .then(a.span.end_line.cmp(&b.span.end_line))
            .then(a.span.start_byte.cmp(&b.span.start_byte))
            .then(a.span.end_byte.cmp(&b.span.end_byte))
            .then(a.fingerprint.cmp(&b.fingerprint))
            .then(a.severity.cmp(&b.severity))
            .then(safety_order(a.safety).cmp(&safety_order(b.safety)))
            .then(a.message.cmp(&b.message))
            .then(a.precondition.cmp(&b.precondition))
    });
}

fn safety_order(safety: SafetyClass) -> u8 {
    match safety {
        SafetyClass::SafeAuto => 0,
        SafetyClass::AnalyzerConfirmed => 1,
        SafetyClass::SafeWithPrecondition => 2,
        SafetyClass::RiskySuggest => 3,
        SafetyClass::LlmOnly => 4,
        SafetyClass::NeverAuto => 5,
    }
}

fn work_order_for_findings(key: RewriteRegionKey, findings: Vec<&Finding>) -> WorkOrder {
    let region = key.region();
    let id = workorder_id_for_region(&key.path, &region);
    let region_fingerprint = region_fingerprint(&key.path, &region);
    let revision_guard = region_revision_guard(&key.path, &region);
    WorkOrder {
        schema: "deslop.workorder/2".to_string(),
        kind: WorkOrderKind::RewriteRegion,
        id,
        path: key.path,
        region,
        region_fingerprint,
        revision_guard,
        findings: findings.into_iter().map(work_order_finding).collect(),
        instruction: "Rewrite the region to address every listed finding that can be resolved without changing behavior or the public API. The safety contract wins if findings conflict. Preserve language and indentation.".to_string(),
        contract: Contract::default(),
    }
}

fn work_order_finding(finding: &Finding) -> WorkOrderFinding {
    WorkOrderFinding {
        rule: finding.rule.to_owned(),
        severity: finding.severity,
        safety: finding.safety,
        message: finding.message.to_owned(),
        precondition: finding.precondition.to_owned(),
    }
}

pub fn characterization_work_order_for(work_order: &WorkOrder) -> WorkOrder {
    WorkOrder {
        schema: "deslop.workorder/2".to_string(),
        kind: WorkOrderKind::NeedsCharacterizationTest,
        id: work_order.id.to_owned(),
        path: work_order.path.to_path_buf(),
        region: work_order.region.clone(),
        region_fingerprint: work_order.region_fingerprint.to_owned(),
        revision_guard: work_order.revision_guard.clone(),
        findings: vec![WorkOrderFinding {
            rule: "needs-characterization-test".to_string(),
            severity: Severity::Major,
            safety: SafetyClass::LlmOnly,
            message: "region has a weak test oracle; generate a characterization test before removal".to_string(),
            precondition: None,
        }],
        instruction: "Write a test that pins the current observable behavior of this exact region. Do not change production behavior. Return deslop.characterization-test/2 JSONL with test_path and test_text; the test must compile and pass against the current unmodified code.".to_string(),
        contract: Contract {
            must_parse: true,
            no_new_public_defs: false,
            keep_error_handling: true,
            max_growth_ratio: 1.0,
            check_cmd: work_order.contract.check_cmd.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_core::{DetectedBy, SafetyClass, Severity, Span};

    fn finding(source: &SourceFile, line: usize, rule: &str, safety: SafetyClass) -> Finding {
        Finding {
            path: source.path.to_path_buf(),
            span: Span::new(
                line,
                line,
                source.line_start_byte(line),
                source.line_end_byte(line),
            ),
            rule: rule.to_string(),
            severity: Severity::Minor,
            safety,
            detected_by: DetectedBy::Idiom,
            message: format!("{rule} message"),
            suggestion: format!("{rule} suggestion"),
            precondition: None,
            edit: None,
            fingerprint: format!("finding-{line}-{rule}"),
        }
    }

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
        assert_eq!(value["schema"], "deslop.workorder/2");
        assert_eq!(value["kind"], "rewrite-region");
        assert!(
            value["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("wo2_"))
        );
        assert!(value.get("path").is_some());
        assert_eq!(value["region"]["start_byte"], 0);
        assert_eq!(value["region"]["end_byte"], source.text.len());
        assert!(value.get("findings").is_some());
        assert!(value.get("instruction").is_some());
        assert!(value.get("contract").is_some());
        assert!(value["region_fingerprint"].is_string());
        assert!(
            value["revision_guard"]
                .as_str()
                .is_some_and(|guard| guard.starts_with("rg1_"))
        );
        validate_workorder_identity(&work_order).expect("valid generated identity");
    }

    #[test]
    fn workorder_identity_survives_outer_whitespace_but_revision_guard_expires() {
        let original = SourceFile::new(PathBuf::from("sample.rs"), "value();\n".into());
        let changed = SourceFile::new(PathBuf::from("sample.rs"), " value();\n".into());
        let original_order = work_orders_for_source(
            &original,
            &[finding(&original, 1, "long-method", SafetyClass::LlmOnly)],
        )
        .remove(0);
        let changed_order = work_orders_for_source(
            &changed,
            &[finding(&changed, 1, "long-method", SafetyClass::LlmOnly)],
        )
        .remove(0);

        assert_eq!(
            original_order.region_fingerprint,
            changed_order.region_fingerprint
        );
        assert_eq!(original_order.id, changed_order.id);
        assert_ne!(original_order.revision_guard, changed_order.revision_guard);
    }

    #[test]
    fn partial_unknown_and_mismatched_reports_cannot_create_workorders() {
        let source = SourceFile::new(
            PathBuf::from("malformed.ts"),
            include_str!("../../../tests/fixtures/typescript/malformed.ts").to_string(),
        );
        let injected = finding(&source, 1, "narrating-comment", SafetyClass::LlmOnly);
        let partial = FileReport {
            path: source.path.clone(),
            lang: source.lang,
            analysis: deslop_parse::analysis_provenance_or_failed(&source),
            findings: vec![injected.clone()],
        };
        let unknown = FileReport {
            analysis: deslop_core::AnalysisProvenance::default(),
            ..partial.clone()
        };
        let mismatched = FileReport {
            path: PathBuf::from("other.ts"),
            analysis: deslop_core::AnalysisProvenance::complete(),
            ..partial.clone()
        };

        assert!(work_orders_for_report(&source, &partial).is_empty());
        assert!(work_orders_for_report(&source, &unknown).is_empty());
        assert!(work_orders_for_report(&source, &mismatched).is_empty());
    }

    #[test]
    fn groups_all_non_safe_findings_in_the_same_enclosing_region() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn noisy() {\n    todo!();\n    println!(\"narration\");\n}\n".into(),
        );
        let findings = vec![
            finding(&source, 3, "narrating-comment", SafetyClass::LlmOnly),
            finding(&source, 2, "placeholder", SafetyClass::RiskySuggest),
            finding(&source, 2, "safe-format", SafetyClass::SafeAuto),
        ];

        let work_orders = work_orders_for_source(&source, &findings);

        assert_eq!(work_orders.len(), 1);
        assert_eq!(work_orders[0].region.start_line, 1);
        assert_eq!(work_orders[0].region.end_line, 4);
        assert_eq!(work_orders[0].findings.len(), 2);
        assert_eq!(work_orders[0].findings[0].rule, "placeholder");
        assert_eq!(work_orders[0].findings[1].rule, "narrating-comment");
    }

    #[test]
    fn typed_tsx_finding_targets_the_enclosing_component() {
        let source = SourceFile::new(
            PathBuf::from("component.tsx"),
            include_str!("../../../tests/fixtures/typescript/component.tsx").to_string(),
        );
        let work_orders = work_orders_for_source(
            &source,
            &[finding(
                &source,
                14,
                "typed-component-cleanup",
                SafetyClass::LlmOnly,
            )],
        );

        assert_eq!(source.lang, deslop_core::Lang::TypeScript);
        assert_eq!(work_orders.len(), 1);
        assert_eq!(work_orders[0].region.start_line, 11);
        assert_eq!(work_orders[0].region.end_line, 21);
        assert!(work_orders[0].region.text.contains("function View"));
    }

    #[test]
    fn python_findings_target_decorated_and_nested_callable_regions() {
        let source = SourceFile::new(
            PathBuf::from("behavioral.py"),
            include_str!("../../../tests/fixtures/python/behavioral.py").to_string(),
        );
        let findings = vec![
            finding(&source, 14, "async-cleanup", SafetyClass::LlmOnly),
            finding(&source, 16, "nested-cleanup", SafetyClass::LlmOnly),
        ];
        let work_orders = work_orders_for_source(&source, &findings);

        assert_eq!(source.lang, deslop_core::Lang::Python);
        assert_eq!(work_orders.len(), 2);
        assert_eq!(work_orders[0].region.start_line, 13);
        assert_eq!(work_orders[0].region.end_line, 18);
        assert!(work_orders[0].region.text.starts_with("    @traced"));
        assert_eq!(work_orders[1].region.start_line, 15);
        assert_eq!(work_orders[1].region.end_line, 16);
        assert!(work_orders[1].region.text.contains("def normalize"));
    }

    #[test]
    fn emits_distinct_unique_orders_for_distinct_regions() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn first() {\n    todo!();\n}\n\nfn second() {\n    todo!();\n}\n".into(),
        );
        let findings = vec![
            finding(&source, 6, "placeholder", SafetyClass::LlmOnly),
            finding(&source, 2, "placeholder", SafetyClass::LlmOnly),
        ];

        let work_orders = work_orders_for_source(&source, &findings);

        assert_eq!(work_orders.len(), 2);
        assert_eq!(work_orders[0].region.start_line, 1);
        assert_eq!(work_orders[1].region.start_line, 5);
        assert_ne!(work_orders[0].id, work_orders[1].id);
    }

    #[test]
    fn grouping_is_invariant_to_finding_input_order() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn noisy() {\n    todo!();\n    println!(\"narration\");\n}\n".into(),
        );
        let mut left = finding(&source, 2, "placeholder", SafetyClass::RiskySuggest);
        left.fingerprint = "shared-fingerprint".to_string();
        left.message = "first message".to_string();
        let mut right = finding(&source, 2, "placeholder", SafetyClass::LlmOnly);
        right.fingerprint = "shared-fingerprint".to_string();
        right.message = "second message".to_string();

        let forward = work_orders_for_source(&source, &[left.clone(), right.clone()]);
        let reversed = work_orders_for_source(&source, &[right, left]);

        assert_eq!(
            serde_json::to_value(forward).expect("forward JSON"),
            serde_json::to_value(reversed).expect("reversed JSON")
        );
    }

    #[test]
    fn source_path_is_the_authoritative_group_and_identity_path() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn noisy() {\n    todo!();\n    println!(\"narration\");\n}\n".into(),
        );
        let direct = finding(&source, 2, "placeholder", SafetyClass::RiskySuggest);
        let mut equivalent = finding(&source, 3, "narration", SafetyClass::LlmOnly);
        equivalent.path = PathBuf::from("./sample.rs");

        let work_orders = work_orders_for_source(&source, &[direct, equivalent]);

        assert_eq!(work_orders.len(), 1);
        assert_eq!(work_orders[0].path, source.path);
        assert_eq!(work_orders[0].findings.len(), 2);
    }

    #[test]
    fn overlapping_nested_callable_regions_remain_distinct_targets() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn outer() {\n    todo!();\n    fn inner() {\n        todo!();\n    }\n}\n".into(),
        );
        let findings = vec![
            finding(&source, 2, "outer-placeholder", SafetyClass::LlmOnly),
            finding(&source, 4, "inner-placeholder", SafetyClass::LlmOnly),
        ];

        let work_orders = work_orders_for_source(&source, &findings);

        assert_eq!(work_orders.len(), 2);
        assert_eq!(
            work_orders
                .iter()
                .map(|work_order| (work_order.region.start_line, work_order.region.end_line))
                .collect::<Vec<_>>(),
            vec![(1, 6), (3, 5)]
        );
        assert_ne!(work_orders[0].id, work_orders[1].id);
    }
}
