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
pub struct Span {
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
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
    pub findings: Vec<Finding>,
}

pub fn fingerprint(path: &Path, rule: &str, span: Span, text: &str) -> String {
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

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

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
            default: "propose",
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
    }
}
