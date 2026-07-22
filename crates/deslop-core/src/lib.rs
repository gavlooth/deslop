use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

macro_rules! kebab_data_enum {
    ($vis:vis enum $name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        $vis enum $name {
            $($variant),+
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Minor,
    Major,
}

impl Severity {
    pub fn passes_threshold(self, threshold: Severity) -> bool {
        self >= threshold
    }
}

impl std::str::FromStr for Severity {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "info" => Ok(Self::Info),
            "minor" => Ok(Self::Minor),
            "major" => Ok(Self::Major),
            other => Err(format!("unknown severity `{other}`")),
        }
    }
}

kebab_data_enum! {
pub enum SafetyClass {
    SafeAuto,
    AnalyzerConfirmed,
    SafeWithPrecondition,
    RiskySuggest,
    LlmOnly,
    NeverAuto,
}
}

impl SafetyClass {
    /// Whether a finding may cross the proposal boundary into an agent rewrite work order.
    ///
    /// `SafeAuto` is handled by deterministic fixes. `NeverAuto` is evidence only: it must
    /// remain visible in reports but can never grant an agent, verifier, or apply path rewrite
    /// authority.
    pub fn permits_proposal(self) -> bool {
        matches!(
            self,
            Self::AnalyzerConfirmed
                | Self::SafeWithPrecondition
                | Self::RiskySuggest
                | Self::LlmOnly
        )
    }
}

kebab_data_enum! {
pub enum DetectedBy {
    Text,
    Idiom,
    Duplication,
    Complexity,
    Boundary,
    CljKondo,
    JuliaAnalyzer,
    RustAnalyzer,
    RefactorHistory,
    RefactorSnapshot,
}
}

kebab_data_enum! {
pub enum Lang {
    Clojure,
    Julia,
    Python,
    JavaScript,
    TypeScript,
    Rust,
    Generic,
}
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Clojure => "clojure",
            Self::Julia => "julia",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Rust => "rust",
            Self::Generic => "generic",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// Exact proposal-time target identity used only to reject stale writes.
///
/// Unlike [`baseline_fingerprint`], this value hashes the raw bytes without trimming. It is
/// intentionally a distinct type so normalized finding identity cannot be passed as write authority.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RevisionGuard(String);

impl RevisionGuard {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for RevisionGuard {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for RevisionGuard {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl fmt::Display for RevisionGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Span {
    pub fn new(start_line: usize, end_line: usize, start_byte: usize, end_byte: usize) -> Self {
        Self {
            start_line,
            end_line,
            start_byte,
            end_byte,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnalysisStatus {
    #[default]
    Unknown,
    Complete,
    Partial,
    Unsupported,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisDiagnostic {
    pub code: String,
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisProvenance {
    pub status: AnalysisStatus,
    pub diagnostics: Vec<AnalysisDiagnostic>,
}

impl Default for AnalysisProvenance {
    fn default() -> Self {
        Self {
            status: AnalysisStatus::Unknown,
            diagnostics: vec![AnalysisDiagnostic {
                code: "analysis-unknown".to_string(),
                message: "analysis provenance is unavailable; rewrite authority is denied"
                    .to_string(),
                span: None,
            }],
        }
    }
}

impl AnalysisProvenance {
    pub fn complete() -> Self {
        Self {
            status: AnalysisStatus::Complete,
            diagnostics: Vec::new(),
        }
    }

    pub fn partial(diagnostics: Vec<AnalysisDiagnostic>) -> Self {
        Self {
            status: AnalysisStatus::Partial,
            diagnostics,
        }
    }

    pub fn unsupported(diagnostics: Vec<AnalysisDiagnostic>) -> Self {
        Self {
            status: AnalysisStatus::Unsupported,
            diagnostics,
        }
    }

    pub fn failed(diagnostics: Vec<AnalysisDiagnostic>) -> Self {
        Self {
            status: AnalysisStatus::Failed,
            diagnostics,
        }
    }

    pub fn permits_rewrites(&self) -> bool {
        self.status == AnalysisStatus::Complete && self.diagnostics.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAnalysis {
    pub path: PathBuf,
    pub lang: Lang,
    pub analysis: AnalysisProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Splice {
    pub start_byte: usize,
    pub end_byte: usize,
    pub replacement: String,
}

kebab_data_enum! {
pub enum EditKind {
    SafeAuto,
    AnalyzerConfirmed,
}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub splices: Vec<Splice>,
    pub kind: EditKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub path: PathBuf,
    pub span: Span,
    pub rule: String,
    pub severity: Severity,
    pub safety: SafetyClass,
    pub detected_by: DetectedBy,
    pub message: String,
    pub suggestion: String,
    pub precondition: Option<String>,
    pub edit: Option<Edit>,
    pub fingerprint: String,
}

impl Finding {
    pub fn loc(&self) -> String {
        if self.span.start_line == self.span.end_line {
            format!("{}:{}", self.path.display(), self.span.start_line)
        } else {
            format!(
                "{}:{}-{}",
                self.path.display(),
                self.span.start_line,
                self.span.end_line
            )
        }
    }

    pub fn is_fixable_by_default(&self) -> bool {
        self.safety == SafetyClass::SafeAuto && self.edit.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReport {
    pub path: PathBuf,
    pub lang: Lang,
    #[serde(default)]
    pub analysis: AnalysisProvenance,
    pub findings: Vec<Finding>,
}

pub fn reports_analysis_status(reports: &[FileReport]) -> AnalysisStatus {
    if reports.is_empty() {
        return AnalysisStatus::Complete;
    }
    if reports
        .iter()
        .all(|report| report.analysis.permits_rewrites())
    {
        return AnalysisStatus::Complete;
    }
    if reports
        .iter()
        .any(|report| report.analysis.status == AnalysisStatus::Failed)
    {
        return AnalysisStatus::Failed;
    }
    if reports
        .iter()
        .any(|report| report.analysis.status == AnalysisStatus::Unknown)
    {
        return AnalysisStatus::Unknown;
    }
    if reports
        .iter()
        .all(|report| report.analysis.status == AnalysisStatus::Unsupported)
    {
        return AnalysisStatus::Unsupported;
    }
    AnalysisStatus::Partial
}

pub fn reports_permit_rewrites(reports: &[FileReport]) -> bool {
    reports
        .iter()
        .all(|report| report.analysis.permits_rewrites())
}

#[cfg(test)]
mod analysis_tests {
    use super::*;

    #[test]
    fn legacy_report_without_provenance_defaults_to_unknown_and_denies_rewrites() {
        let report: FileReport =
            serde_json::from_str(r#"{"path":"sample.ts","lang":"type-script","findings":[]}"#)
                .expect("legacy report");

        assert_eq!(report.analysis.status, AnalysisStatus::Unknown);
        assert_eq!(report.analysis.diagnostics[0].code, "analysis-unknown");
        assert!(!report.analysis.permits_rewrites());
        assert!(!reports_permit_rewrites(&[report]));
    }

    #[test]
    fn inconsistent_complete_provenance_with_diagnostics_denies_rewrites() {
        let analysis = AnalysisProvenance {
            status: AnalysisStatus::Complete,
            diagnostics: vec![AnalysisDiagnostic {
                code: "unexpected".to_string(),
                message: "complete analysis cannot carry diagnostics".to_string(),
                span: None,
            }],
        };

        assert!(!analysis.permits_rewrites());
    }
}

#[cfg(test)]
mod safety_tests {
    use super::SafetyClass;

    #[test]
    fn proposal_authority_excludes_deterministic_and_report_only_classes() {
        assert!(!SafetyClass::SafeAuto.permits_proposal());
        assert!(SafetyClass::AnalyzerConfirmed.permits_proposal());
        assert!(SafetyClass::SafeWithPrecondition.permits_proposal());
        assert!(SafetyClass::RiskySuggest.permits_proposal());
        assert!(SafetyClass::LlmOnly.permits_proposal());
        assert!(!SafetyClass::NeverAuto.permits_proposal());
    }
}

pub fn file_analyses_status(analyses: &[FileAnalysis]) -> AnalysisStatus {
    if analyses.is_empty() {
        return AnalysisStatus::Complete;
    }
    if analyses.iter().all(|file| file.analysis.permits_rewrites()) {
        return AnalysisStatus::Complete;
    }
    if analyses
        .iter()
        .any(|file| file.analysis.status == AnalysisStatus::Failed)
    {
        return AnalysisStatus::Failed;
    }
    if analyses
        .iter()
        .any(|file| file.analysis.status == AnalysisStatus::Unknown)
    {
        return AnalysisStatus::Unknown;
    }
    if analyses
        .iter()
        .all(|file| file.analysis.status == AnalysisStatus::Unsupported)
    {
        return AnalysisStatus::Unsupported;
    }
    AnalysisStatus::Partial
}

/// Best-effort finding identity for baseline matching across revisions.
///
/// This preserves the original `deslop.baseline/1` algorithm, including outer-whitespace trimming.
/// It must never be used to authorize a write.
pub fn baseline_fingerprint(path: &Path, rule: &str, span: Span, text: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let normalized_path = normalize_path(path);
    let start_line = span.start_line.to_string();
    let end_line = span.end_line.to_string();
    for part in [
        normalized_path.as_bytes(),
        rule.as_bytes(),
        start_line.as_bytes(),
        end_line.as_bytes(),
        text.trim().as_bytes(),
    ] {
        for byte in part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Build a collision-resistant guard over the exact target bytes and their source identity.
pub fn revision_guard(path: &Path, span: Span, exact_text: &str) -> RevisionGuard {
    const CONTEXT: &str = "deslop 2026-07-13 revision guard v1";
    let mut hasher = blake3::Hasher::new_derive_key(CONTEXT);
    let mut update = |part: &[u8]| {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part);
    };
    let normalized_path = normalize_path(path);
    update(normalized_path.as_bytes());
    update(&span.start_line.to_le_bytes());
    update(&span.end_line.to_le_bytes());
    update(&span.start_byte.to_le_bytes());
    update(&span.end_byte.to_le_bytes());
    update(exact_text.as_bytes());
    RevisionGuard(format!("rg1_{}_{}", exact_text.len(), hasher.finalize()))
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn baseline_identity_trims_outer_whitespace_but_revision_guard_does_not() {
        let path = Path::new("./src/sample.rs");
        let span = Span::new(4, 4, 20, 31);
        let original = "value();\n";
        let baseline = baseline_fingerprint(path, "rule", span, original);
        let guard = revision_guard(path, span, original);

        for changed in [
            " value();\n",
            "\tvalue();\n",
            "value(); \n",
            "value();\t\n",
            "value();",
            "value();\r\n",
            "\u{2003}value();\n",
        ] {
            assert_eq!(
                baseline_fingerprint(path, "rule", span, changed),
                baseline,
                "baseline identity should survive outer whitespace: {changed:?}"
            );
            assert_ne!(
                revision_guard(path, span, changed),
                guard,
                "revision guard must bind exact bytes: {changed:?}"
            );
        }
    }

    #[test]
    fn revision_guard_is_domain_separated_from_baseline_identity() {
        let path = Path::new("src/sample.rs");
        let span = Span::new(1, 1, 0, 9);
        let baseline = baseline_fingerprint(path, "region", span, "value();\n");
        let guard = revision_guard(path, span, "value();\n");

        assert!(guard.as_str().starts_with("rg1_9_"));
        assert_ne!(guard.as_str(), baseline);
        assert_eq!(guard, revision_guard(path, span, "value();\n"));
    }
}

pub mod refactor_defect;
pub mod snapshot_pathology;

/// The single canonical registry of every rule deslop can emit.
///
/// This is the one source of truth shared by:
/// - suppression validation ([`rules::is_known`]),
/// - the `deslop rules` CLI output and MCP `rules` tool ([`rules::render_table`]).
///
/// Keep new rules in sync here so the surfaces above cannot drift apart.
pub mod rules {
    /// One row of the rule registry: identifier plus the user-facing safety and default-action
    /// labels shown by `deslop rules`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct RuleInfo {
        pub name: &'static str,
        pub safety: &'static str,
        pub default: &'static str,
    }

    /// Every rule deslop can emit, including rules surfaced through external analyzers.
    pub const RULES: &[RuleInfo] = &[
        RuleInfo {
            name: "consecutive-blank-lines",
            safety: "safe-auto",
            default: "fix",
        },
        RuleInfo {
            name: "reimpl-not=",
            safety: "safe-auto",
            default: "fix",
        },
        RuleInfo {
            name: "reimpl-some?",
            safety: "safe-auto",
            default: "fix",
        },
        RuleInfo {
            name: "reimpl-boolean",
            safety: "safe-auto",
            default: "fix",
        },
        RuleInfo {
            name: "redundant-do",
            safety: "safe-auto",
            default: "fix",
        },
        RuleInfo {
            name: "reimpl-empty?",
            safety: "safe-with-precondition",
            default: "suggest (finite/countable collection)",
        },
        RuleInfo {
            name: "reimpl-seq",
            safety: "safe-with-precondition",
            default: "suggest (finite/countable collection)",
        },
        RuleInfo {
            name: "reimpl-vec",
            safety: "safe-with-precondition",
            default: "suggest (finite collection)",
        },
        RuleInfo {
            name: "reimpl-isempty",
            safety: "safe-with-precondition",
            default: "suggest (standard collection semantics)",
        },
        RuleInfo {
            name: "reimpl-eachindex",
            safety: "safe-with-precondition",
            default: "suggest (same collection indexing, not ordinal use)",
        },
        RuleInfo {
            name: "reimpl-isnothing",
            safety: "risky-suggest",
            default: "suggest",
        },
        RuleInfo {
            name: "useless-format",
            safety: "safe-with-precondition",
            default: "suggest (Display equivalent to ToString)",
        },
        RuleInfo {
            name: "py-none-comparison",
            safety: "safe-with-precondition",
            default: "suggest",
        },
        RuleInfo {
            name: "py-range-len",
            safety: "risky-suggest",
            default: "suggest",
        },
        RuleInfo {
            name: "py-dict-keys-membership",
            safety: "safe-with-precondition",
            default: "suggest",
        },
        RuleInfo {
            name: "py-list-comprehension-wrapper",
            safety: "risky-suggest",
            default: "suggest",
        },
        RuleInfo {
            name: "js-loose-equality",
            safety: "safe-with-precondition",
            default: "suggest",
        },
        RuleInfo {
            name: "js-var-declaration",
            safety: "safe-with-precondition",
            default: "suggest",
        },
        RuleInfo {
            name: "js-unnecessary-await",
            safety: "risky-suggest",
            default: "suggest",
        },
        RuleInfo {
            name: "redundant-closure",
            safety: "risky-suggest",
            default: "suggest",
        },
        RuleInfo {
            name: "let-and-return",
            safety: "risky-suggest",
            default: "suggest",
        },
        RuleInfo {
            name: "needless-clone",
            safety: "llm-only",
            default: "propose",
        },
        RuleInfo {
            name: "needless-return",
            safety: "analyzer-confirmed",
            default: "fix only with clippy confirmation",
        },
        RuleInfo {
            name: "unused-arg",
            safety: "analyzer-confirmed",
            default: "fix only with StaticLint confirmation",
        },
        RuleInfo {
            name: "unused-binding",
            safety: "analyzer-confirmed",
            default: "fix only with external analyzer confirmation",
        },
        RuleInfo {
            name: "unused-private-def",
            safety: "analyzer-confirmed",
            default: "fix only with clj-kondo confirmation",
        },
        RuleInfo {
            name: "unused-namespace",
            safety: "analyzer-confirmed",
            default: "fix only with clj-kondo confirmation",
        },
        RuleInfo {
            name: "missing-reference",
            safety: "never-auto",
            default: "review unresolved reference (report only)",
        },
        RuleInfo {
            name: "julia-jet",
            safety: "never-auto",
            default: "review correctness diagnostic (report only)",
        },
        RuleInfo {
            name: "single-use-binding",
            safety: "risky-suggest",
            default: "suggest",
        },
        RuleInfo {
            name: "incompleteness",
            safety: "llm-only",
            default: "propose",
        },
        RuleInfo {
            name: "magic-number",
            safety: "risky-suggest",
            default: "suggest",
        },
        RuleInfo {
            name: "long-method",
            safety: "llm-only",
            default: "propose",
        },
        RuleInfo {
            name: "slop-score",
            safety: "report",
            default: "deslop slop",
        },
        RuleInfo {
            name: "narrating-comment",
            safety: "llm-only",
            default: "propose",
        },
        RuleInfo {
            name: "comment-block",
            safety: "llm-only",
            default: "propose",
        },
        RuleInfo {
            name: "duplicate-block",
            safety: "llm-only",
            default: "propose",
        },
        RuleInfo {
            name: "near-duplicate",
            safety: "llm-only",
            default: "propose",
        },
        RuleInfo {
            name: "config-key-unread",
            safety: "never-auto",
            default: "review (declared config key no code reads)",
        },
        RuleInfo {
            name: "config-key-unconsumed",
            safety: "never-auto",
            default: "review (parsed+echoed but nothing behavioral consumes it)",
        },
        RuleInfo {
            name: "config-key-shadowed",
            safety: "never-auto",
            default: "review (parsed value overwritten by a literal)",
        },
        RuleInfo {
            name: "owner-moved-consumer-stale",
            safety: "never-auto",
            default: "review (owner moved; consumer still derives from former owner)",
        },
        RuleInfo {
            name: "scope-collapse-after-refactor",
            safety: "never-auto",
            default: "review (partitioned operation became global after reshape)",
        },
        RuleInfo {
            name: "mechanism-live-gate-retired",
            safety: "never-auto",
            default: "review (live mechanism; gate still reads retired value)",
        },
        RuleInfo {
            name: "producer-verifier-schema-drift",
            safety: "never-auto",
            default: "review (producer changed; verifier or reader did not)",
        },
        RuleInfo {
            name: "accepted-config-inert",
            safety: "never-auto",
            default: "review (accepted parameter no longer reaches behavior)",
        },
        RuleInfo {
            name: "confidence-provenance-lost",
            safety: "never-auto",
            default: "review (public score reconstructed from lossy representation)",
        },
        RuleInfo {
            name: "telemetry-not-bound-to-claim",
            safety: "never-auto",
            default: "review (metric not bound to the claimed mechanism)",
        },
        RuleInfo {
            name: "test-oracle-lag",
            safety: "never-auto",
            default: "review (tests do not exercise the changed contract)",
        },
        RuleInfo {
            name: "hot-path-work-duplicated",
            safety: "never-auto",
            default: "review (equivalent expensive computation duplicated)",
        },
        RuleInfo {
            name: "operational-identity-stale",
            safety: "never-auto",
            default: "review (status surfaces a replaced identity)",
        },
        RuleInfo {
            name: "adoption-chain-incomplete",
            safety: "never-auto",
            default: "review (summary of an incomplete owner-migration chain)",
        },
        RuleInfo {
            name: "owner-consumer-contract-split",
            safety: "never-auto",
            default: "review (current owner and consumer paths disagree)",
        },
        RuleInfo {
            name: "partition-boundary-not-preserved",
            safety: "never-auto",
            default: "review (current partition boundary may be crossed)",
        },
        RuleInfo {
            name: "mechanism-gate-contract-split",
            safety: "never-auto",
            default: "review (current mechanism and gate paths disagree)",
        },
        RuleInfo {
            name: "producer-verifier-schema-mismatch",
            safety: "never-auto",
            default: "review (current producer and verifier schemas disagree)",
        },
        RuleInfo {
            name: "accepted-config-no-behavioral-reach",
            safety: "never-auto",
            default: "review (accepted config has no observed behavioral reach)",
        },
        RuleInfo {
            name: "confidence-derived-after-lossy-commit",
            safety: "never-auto",
            default: "review (public confidence follows a lossy operation)",
        },
        RuleInfo {
            name: "telemetry-claim-unbound",
            safety: "never-auto",
            default: "review (telemetry and claimed mechanism paths disagree)",
        },
        RuleInfo {
            name: "test-contract-dimension-uncovered",
            safety: "never-auto",
            default: "review (current contract dimension lacks an observed oracle)",
        },
        RuleInfo {
            name: "same-path-expensive-work-repeated",
            safety: "never-auto",
            default: "review (composite work repeats on one current path)",
        },
        RuleInfo {
            name: "published-identity-not-live",
            safety: "never-auto",
            default: "review (published and governing identity paths disagree)",
        },
        RuleInfo {
            name: "contract-chain-incomplete",
            safety: "never-auto",
            default: "review (summary of current contract-path splits)",
        },
    ];

    /// Whether `name` is a rule deslop knows how to emit.
    pub fn is_known(name: &str) -> bool {
        RULES.iter().any(|rule| rule.name == name)
    }

    /// Comma-separated list of every rule name, for error messages.
    pub fn names_csv() -> String {
        RULES
            .iter()
            .map(|rule| rule.name)
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Render the rule registry as the aligned text table shown by `deslop rules`.
    pub fn render_table() -> String {
        // Each column is as wide as its longest cell, but never narrower than its header.
        let longest = |cell: fn(&RuleInfo) -> &str, header: &str| {
            RULES
                .iter()
                .map(|rule| cell(rule).len())
                .max()
                .unwrap_or(0)
                .max(header.len())
                + 2
        };
        let name_width = longest(|rule| rule.name, "rule");
        let safety_width = longest(|rule| rule.safety, "safety");
        let mut out = format!(
            "{:name_width$}{:safety_width$}{}\n",
            "rule", "safety", "default"
        );
        for rule in RULES {
            out.push_str(&format!(
                "{:name_width$}{:safety_width$}{}\n",
                rule.name, rule.safety, rule.default
            ));
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn registry_has_no_duplicate_names() {
            let mut names: Vec<&str> = RULES.iter().map(|rule| rule.name).collect();
            names.sort_unstable();
            let count = names.len();
            names.dedup();
            assert_eq!(names.len(), count, "duplicate rule name in RULES");
        }

        #[test]
        fn is_known_matches_registry() {
            assert!(is_known("magic-number"));
            assert!(is_known("near-duplicate"));
            assert!(!is_known("ignore_comments"));
        }

        #[test]
        fn render_table_lists_every_rule() {
            let table = render_table();
            assert!(table.starts_with("rule"));
            for rule in RULES {
                assert!(table.contains(rule.name), "missing {} in table", rule.name);
            }
        }

        #[test]
        fn never_auto_rules_never_advertise_rewrite_authority() {
            for rule in RULES.iter().filter(|rule| rule.safety == "never-auto") {
                assert!(
                    !rule.default.contains("propose") && !rule.default.contains("fix"),
                    "never-auto rule {} advertises rewrite authority: {}",
                    rule.name,
                    rule.default
                );
            }
        }
    }
}
