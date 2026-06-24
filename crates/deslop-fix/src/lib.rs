use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use deslop_analyzer::scan_paths;
use deslop_core::{FileReport, Finding, SafetyClass, Splice};

#[derive(Debug, Clone, Copy)]
pub struct FixOptions {
    pub dry_run: bool,
    pub backup: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixOutcome {
    pub path: PathBuf,
    pub applied: usize,
    pub changed: bool,
}

pub fn fix_paths(paths: &[PathBuf], options: FixOptions) -> Result<Vec<FixOutcome>> {
    let reports = scan_paths(paths)?;
    let by_path = fixable_findings_by_path(reports);
    let mut outcomes = Vec::new();
    for (path, findings) in by_path {
        outcomes.push(apply_fix_to_path(path, findings, options)?);
    }
    Ok(outcomes)
}

pub fn diff_paths(paths: &[PathBuf]) -> Result<String> {
    let reports = scan_paths(paths)?;
    let by_path = fixable_findings_by_path(reports);
    let mut out = String::new();
    for (path, findings) in by_path {
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let fixed = apply_findings_to_text(&text, &findings)?;
        if fixed != text {
            out.push_str(&unified_file_diff(&path, &text, &fixed));
        }
    }
    Ok(out)
}

fn fixable_findings_by_path(reports: Vec<FileReport>) -> BTreeMap<PathBuf, Vec<Finding>> {
    let mut by_path = BTreeMap::new();
    for report in reports {
        let fixable = report
            .findings
            .into_iter()
            .filter(is_fixable_finding)
            .collect::<Vec<_>>();
        if !fixable.is_empty() {
            by_path.insert(report.path, fixable);
        }
    }
    by_path
}

fn apply_fix_to_path(
    path: PathBuf,
    findings: Vec<Finding>,
    options: FixOptions,
) -> Result<FixOutcome> {
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let fixed = apply_findings_to_text(&text, &findings)?;
    let changed = fixed != text;
    if changed && !options.dry_run {
        write_changed_text(&path, &text, fixed, options.backup)?;
    }
    Ok(FixOutcome {
        path,
        applied: findings.len(),
        changed,
    })
}

fn write_changed_text(path: &Path, original: &str, fixed: String, backup: bool) -> Result<()> {
    if backup {
        let backup = backup_path(path);
        fs::write(&backup, original)
            .with_context(|| format!("failed to write {}", backup.display()))?;
    }
    let tmp = temp_path(path);
    fs::write(&tmp, fixed).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "failed to replace {} with {}",
            path.display(),
            tmp.display()
        )
    })
}

fn unified_file_diff(path: &Path, original: &str, fixed: &str) -> String {
    let original_lines = original.lines().count();
    let fixed_lines = fixed.lines().count();
    let mut out = String::new();
    out.push_str(&format!("--- {}\n", path.display()));
    out.push_str(&format!("+++ {}\n", path.display()));
    out.push_str(&format!("@@ -1,{original_lines} +1,{fixed_lines} @@\n"));
    for line in original.lines() {
        out.push('-');
        out.push_str(line);
        out.push('\n');
    }
    for line in fixed.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn temp_path(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}deslop.tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!("{ext}."))
            .unwrap_or_default()
    ))
}

fn is_fixable_finding(finding: &Finding) -> bool {
    matches!(
        finding.safety,
        SafetyClass::SafeAuto | SafetyClass::AnalyzerConfirmed
    ) && finding
        .edit
        .as_ref()
        .is_some_and(|edit| !edit.splices.is_empty())
}

pub fn apply_findings_to_text(text: &str, findings: &[Finding]) -> Result<String> {
    let mut splices: Vec<Splice> = findings
        .iter()
        .filter(|finding| is_fixable_finding(finding))
        .filter_map(|finding| finding.edit.as_ref())
        .flat_map(|edit| edit.splices.iter().cloned())
        .collect();
    splices.sort_by(|a, b| {
        b.start_byte
            .cmp(&a.start_byte)
            .then(b.end_byte.cmp(&a.end_byte))
    });

    let mut last_start = text.len() + 1;
    let mut out = text.to_string();
    for splice in splices {
        if splice.start_byte > splice.end_byte || splice.end_byte > out.len() {
            bail!(
                "invalid splice {}..{} for {} bytes",
                splice.start_byte,
                splice.end_byte,
                out.len()
            );
        }
        if splice.end_byte > last_start {
            bail!("overlapping safe-auto splices");
        }
        out.replace_range(splice.start_byte..splice.end_byte, &splice.replacement);
        last_start = splice.start_byte;
    }
    Ok(out)
}

pub fn undo_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut restored = Vec::new();
    let paths = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    for path in paths {
        if path.is_dir() {
            for entry in ignore::WalkBuilder::new(&path)
                .hidden(false)
                .filter_entry(|entry| entry.file_name() != ".jj" && entry.file_name() != ".git")
                .build()
            {
                let entry = entry?;
                if entry.file_type().is_some_and(|kind| kind.is_file())
                    && entry.path().to_string_lossy().ends_with(".deslop.bak")
                {
                    restore_backup(entry.path(), &mut restored)?;
                }
            }
        } else {
            let backup = backup_path(&path);
            if backup.exists() {
                restore_backup(&backup, &mut restored)?;
            }
        }
    }
    Ok(restored)
}

fn restore_backup(backup: &Path, restored: &mut Vec<PathBuf>) -> Result<()> {
    let original = original_path_from_backup(backup)?;
    fs::rename(backup, &original).with_context(|| {
        format!(
            "failed to restore {} to {}",
            backup.display(),
            original.display()
        )
    })?;
    restored.push(original);
    Ok(())
}

pub fn backup_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.deslop.bak", path.display()))
}

fn original_path_from_backup(path: &Path) -> Result<PathBuf> {
    let value = path.to_string_lossy();
    let Some(original) = value.strip_suffix(".deslop.bak") else {
        bail!("not a deslop backup: {}", path.display());
    };
    Ok(PathBuf::from(original))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_analyzer::scan_source;
    use deslop_parse::SourceFile;

    #[test]
    fn applies_safe_auto_clojure_splice() {
        let text = "(def ok (not (= a b)))\n(= (count xs) 0)\n";
        let source = SourceFile::new(PathBuf::from("sample.clj"), text.into());
        let report = scan_source(&source);
        let fixed = apply_findings_to_text(text, &report.findings).expect("fixed");
        assert!(fixed.contains("(not= a b)"));
        assert!(fixed.contains("(= (count xs) 0)"));
    }

    #[test]
    fn renders_safe_auto_diff_without_writing() {
        let diff = unified_file_diff(Path::new("sample.clj"), "(not (= a b))\n", "(not= a b)\n");
        assert!(diff.contains("--- sample.clj"));
        assert!(diff.contains("-(not (= a b))"));
        assert!(diff.contains("+(not= a b)"));
    }
}
