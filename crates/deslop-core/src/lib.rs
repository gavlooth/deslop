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
