use std::path::PathBuf;

use deslop_core::{Finding, Lang, SafetyClass};
use deslop_parse::SourceFile;

use super::*;

fn source(path: &str, text: &str) -> SourceFile {
    SourceFile::new(PathBuf::from(path), text.into())
}

fn clojure_source(text: &str) -> SourceFile {
    source("sample.clj", text)
}

fn duplication_report(text: &str) -> FileReport {
    scan_source_with_config(
        &clojure_source(text),
        AnalyzerConfig {
            min_duplication_tokens: 14,
            ..AnalyzerConfig::default()
        },
    )
}

fn has_rule(report: &FileReport, rule: &str) -> bool {
    report.findings.iter().any(|finding| finding.rule == rule)
}

fn finding_for_rule<'a>(report: &'a FileReport, rule: &str) -> &'a Finding {
    report
        .findings
        .iter()
        .find(|finding| finding.rule == rule)
        .unwrap_or_else(|| panic!("expected {rule} finding"))
}

fn duplicate_rules(report: &FileReport) -> Vec<&str> {
    report
        .findings
        .iter()
        .filter_map(|finding| match finding.rule.as_str() {
            "duplicate-block" | "near-duplicate" => Some(finding.rule.as_str()),
            _ => None,
        })
        .collect()
}

fn long_method_source(nloc: usize) -> SourceFile {
    let mut text = String::from("fn longish() {\n");
    for idx in 0..nloc.saturating_sub(2) {
        text.push_str(&format!("    let _v{idx} = {idx};\n"));
    }
    text.push_str("}\n");
    source("sample.rs", &text)
}

#[test]
fn analyzer_config_defaults_preserve_thresholds() {
    let config = AnalyzerConfig::default();
    assert_eq!(config.min_duplication_tokens, 24);
    assert_eq!(config.long_method_nloc, 40);
    assert_eq!(config.min_meaningful_tokens, 8);
}

#[test]
fn clojure_safe_auto_rules_have_edits() {
    let source = clojure_source(
        "(def a (not (= x y)))\n(def b (not (nil? z)))\n(def c (if p true false))\n",
    );
    let report = scan_source(&source);
    let rules: Vec<_> = report.findings.iter().map(|f| f.rule.as_str()).collect();
    assert!(rules.contains(&"reimpl-not="));
    assert!(rules.contains(&"reimpl-some?"));
    assert!(rules.contains(&"reimpl-boolean"));
    assert!(
        report
            .findings
            .iter()
            .filter(|f| f.safety == SafetyClass::SafeAuto)
            .all(|f| f.edit.is_some())
    );
}

#[test]
fn clojure_count_empty_is_not_safe_auto() {
    let source = clojure_source("(= (count xs) 0)\n");
    let report = scan_source(&source);
    let finding = finding_for_rule(&report, "reimpl-empty?");
    assert_eq!(finding.safety, SafetyClass::SafeWithPrecondition);
    assert!(finding.edit.is_none());
}

#[test]
fn blank_runs_are_safe_auto() {
    let source = source("sample.py", "a = 1\n\n\nb = 2\n");
    let report = scan_source(&source);
    let finding = finding_for_rule(&report, "consecutive-blank-lines");
    assert_eq!(finding.safety, SafetyClass::SafeAuto);
    assert!(finding.edit.is_some());
}

#[test]
fn token_duplication_detects_exact_clone() {
    let report = duplication_report(
        "(defn a [xs]\n  (let [positive (filter pos? xs) doubled (map inc positive) total (reduce + 0 doubled)]\n    (if (> total 10) (+ total 3) (- total 1))))\n(defn a [xs]\n  (let [positive (filter pos? xs) doubled (map inc positive) total (reduce + 0 doubled)]\n    (if (> total 10) (+ total 3) (- total 1))))\n",
    );
    assert!(has_rule(&report, "duplicate-block"));
}

#[test]
fn token_duplication_detects_renamed_clone() {
    let report = duplication_report(
        "(defn a [xs]\n  (let [positive (filter pos? xs) doubled (map inc positive) total (reduce + 0 doubled)]\n    (if (> total 10) (+ total 3) (- total 1))))\n(defn b [items]\n  (let [positive (filter pos? items) doubled (map inc positive) total (reduce + 0 doubled)]\n    (if (> total 10) (+ total 3) (- total 1))))\n",
    );
    assert!(has_rule(&report, "near-duplicate"));
}

#[test]
fn token_duplication_ignores_non_clones() {
    let report = duplication_report(
        "(defn a [x] (let [y (+ x 1)] (* y y)))\n(defn b [z] (let [q (- z 1)] (/ q 2)))\n",
    );
    assert!(
        report
            .findings
            .iter()
            .all(|finding| !matches!(finding.rule.as_str(), "duplicate-block" | "near-duplicate"))
    );
}

#[test]
fn lowered_meaningful_token_floor_enables_smaller_duplicate_finding() {
    let fixture = source(
        "sample.rs",
        "fn a() {\n    if true {\n    }\n}\nfn b() {\n    if true {\n    }\n}\n",
    );
    let default = scan_source_with_config(
        &fixture,
        AnalyzerConfig {
            min_duplication_tokens: 6,
            ..AnalyzerConfig::default()
        },
    );
    assert!(duplicate_rules(&default).is_empty());

    let lowered = scan_source_with_config(
        &fixture,
        AnalyzerConfig {
            min_duplication_tokens: 6,
            min_meaningful_tokens: 1,
            ..AnalyzerConfig::default()
        },
    );
    assert_eq!(duplicate_rules(&lowered), vec!["duplicate-block"]);
}

#[test]
fn lowered_long_method_threshold_flags_below_default_nloc() {
    let fixture = long_method_source(39);
    let default = scan_source(&fixture);
    assert!(!has_rule(&default, "long-method"));

    let lowered = scan_source_with_config(
        &fixture,
        AnalyzerConfig {
            long_method_nloc: 20,
            ..AnalyzerConfig::default()
        },
    );
    assert!(has_rule(&lowered, "long-method"));
}

#[test]
fn fp_corpus_clean_structural_code_has_no_duplication_findings() {
    for (path, text) in [
        (
            "tests/fixtures/clean/structural.rs",
            include_str!("../../../tests/fixtures/clean/structural.rs"),
        ),
        (
            "tests/fixtures/clean/precision_fp.rs",
            include_str!("../../../tests/fixtures/clean/precision_fp.rs"),
        ),
        (
            "tests/fixtures/clean/structural.clj",
            include_str!("../../../tests/fixtures/clean/structural.clj"),
        ),
        (
            "tests/fixtures/clean/structural.jl",
            include_str!("../../../tests/fixtures/clean/structural.jl"),
        ),
    ] {
        let source = source(path, text);
        let report = scan_source(&source);
        assert_eq!(
            duplicate_rules(&report),
            Vec::<&str>::new(),
            "{path} should not have structural duplication findings"
        );
    }
}

#[test]
fn tp_corpus_behavioral_duplicates_still_flagged() {
    for (path, text) in [
        (
            "tests/fixtures/dup/behavior.rs",
            include_str!("../../../tests/fixtures/dup/behavior.rs"),
        ),
        (
            "tests/fixtures/dup/behavior.clj",
            include_str!("../../../tests/fixtures/dup/behavior.clj"),
        ),
        (
            "tests/fixtures/dup/behavior.jl",
            include_str!("../../../tests/fixtures/dup/behavior.jl"),
        ),
    ] {
        let source = source(path, text);
        let report = scan_source(&source);
        assert!(
            !duplicate_rules(&report).is_empty(),
            "{path} should retain behavioral duplication findings"
        );
    }
}

#[test]
fn registry_discovers_registered_test_pack_through_scan_without_core_matches() {
    let mut lang_registry = LangRegistry::new(&deslop_lang::GENERIC_PACK);
    lang_registry.register(&crate::test_pack::TEST_LANG_PACK);
    let mut analyzer_registry = AnalyzerRegistry::new();
    analyzer_registry.register(&crate::test_pack::TEST_ANALYSIS_PACK);

    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("demo.testpack");
    std::fs::write(&path, "# return value\nanything\n").expect("write fixture");
    assert_eq!(lang_registry.pack_for_path(&path).lang(), Lang::Generic);
    assert!(
        deslop_parse::parse_tree(Lang::Generic, "anything\n")
            .expect("parse fallback")
            .is_none()
    );

    let report = scan_file_with_registries(
        &path,
        &lang_registry,
        &analyzer_registry,
        AnalyzerConfig::default(),
    )
    .expect("scan");
    assert_eq!(report.lang, Lang::Generic);
    assert!(has_rule(&report, "test-pack-rule"));
    assert!(has_rule(&report, "narrating-comment"));
}

#[test]
fn rust_idiom_detected_and_fix_withheld_without_check_cmd() {
    let source = source("sample.rs", "fn f() -> i32 {\n    return 1;\n}\n");
    let report = scan_source(&source);
    let finding = finding_for_rule(&report, "needless-return");
    assert_eq!(finding.safety, SafetyClass::SafeWithPrecondition);
    assert!(finding.edit.is_none());
}

#[test]
fn rust_non_tail_return_is_not_needless() {
    let source = source(
        "sample.rs",
        "fn f(x: i32) -> i32 {\n    if x < 0 {\n        return 0;\n    }\n    x + 1\n}\n",
    );
    let report = scan_source(&source);
    assert!(
        !has_rule(&report, "needless-return"),
        "early return inside a branch is not a removable tail expression"
    );
}

#[test]
fn rust_redundant_closure_only_flags_exact_forwarding_call_body() {
    let fixture = source(
        "sample.rs",
        "fn foo(x: i32) -> i32 { x }\nfn process(item: i32) -> i32 { item }\nfn f(xs: Vec<i32>) -> Vec<i32> {\n    xs.into_iter().map(|x| foo(x)).map(|item| process(item)).collect()\n}\n",
    );
    let report = scan_source(&fixture);
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| finding.rule == "redundant-closure")
            .count(),
        2
    );

    let clean = source(
        "sample.rs",
        "fn f(xs: Vec<i32>, y: i32) {\n    let _ = xs.iter().find(|name| Some(name) == xs.first());\n    let _ = xs.iter().map(|x| foo(x).await);\n    let _ = xs.iter().map(|x| foo(x)?);\n    let _ = xs.iter().map(|x| foo(x).method());\n    let _ = xs.iter().map(|x| foo(x, y));\n    let _ = xs.iter().map(|a| foo(y));\n}\n",
    );
    let report = scan_source(&clean);
    assert!(
        !has_rule(&report, "redundant-closure"),
        "wrapping/comparison/multi-arg/trailing-operation closures must not fire"
    );
}

#[test]
fn rust_needless_clone_only_flags_clone_then_borrow_or_iterate() {
    let fixture = source(
        "sample.rs",
        "fn f(v: Vec<String>) {\n    let _ = &v.clone();\n    let _ = v.clone().iter();\n    let _ = v.clone().into_iter();\n    let _ = v.clone().iter_mut();\n}\n",
    );
    let report = scan_source(&fixture);
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| finding.rule == "needless-clone")
            .count(),
        4
    );

    let clean = source(
        "sample.rs",
        "struct Item { field: String }\nfn f(x: String, mut vec: Vec<String>) -> String {\n    let _item = Item { field: x.clone() };\n    if vec.is_empty() {\n        return x.clone();\n    }\n    vec.push(x.clone());\n    let y = x.clone();\n    y\n}\n",
    );
    let report = scan_source(&clean);
    assert!(
        !has_rule(&report, "needless-clone"),
        "owned bare clones must not be reported as needless"
    );
}

#[test]
fn julia_external_is_config_gated_pack_local() {
    assert!(
        JULIA_PACK
            .external_analyzer(&AnalyzerConfig::default())
            .is_none()
    );
    assert!(
        JULIA_PACK
            .external_analyzer(&AnalyzerConfig {
                julia_external: JuliaExternal::StaticLint,
                ..AnalyzerConfig::default()
            })
            .is_some()
    );
}

#[test]
fn julia_staticlint_degrade_keeps_t1_findings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("sample.jl");
    std::fs::write(&path, "x = nothing\nif x == nothing\n    println(x)\nend\n")
        .expect("write fixture");
    let report = scan_file_with_config(
        &path,
        AnalyzerConfig {
            julia_external: JuliaExternal::StaticLint,
            ..AnalyzerConfig::default()
        },
    )
    .expect("scan succeeds even without StaticLint");
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.rule == "reimpl-isnothing")
    );
}

#[test]
fn incompleteness_ignores_stub_words_inside_strings_and_comments() {
    // Trigger words inside a string literal or comment must NOT fire
    // (this was a self-referential false positive on the rule's own pattern).
    let clean = source(
        "clean.rs",
        "fn f() -> i32 {\n    let msg = \"TODO: implement later\"; // placeholder note\n    println!(\"{}\", msg);\n    7\n}\n",
    );
    let report = scan_source_with_config(&clean, AnalyzerConfig::default());
    assert!(
        !has_rule(&report, "incompleteness"),
        "stub words inside strings/comments must not be flagged"
    );

    // A real macro stub in code MUST still fire.
    let stubbed = source("stub.rs", "fn f() {\n    todo!()\n}\n");
    let report = scan_source_with_config(&stubbed, AnalyzerConfig::default());
    assert!(
        has_rule(&report, "incompleteness"),
        "a real todo!() stub must still be flagged"
    );
}
