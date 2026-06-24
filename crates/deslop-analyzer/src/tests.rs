use std::path::PathBuf;

use deslop_core::{Finding, Lang, SafetyClass};
use deslop_parse::SourceFile;

use super::*;

type TextFixture = (&'static str, &'static str);

const CLEAN_DUPLICATION_FIXTURES: &[TextFixture] = &[
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
];

const BEHAVIORAL_DUPLICATION_FIXTURES: &[TextFixture] = &[
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
];

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

fn assert_duplication_findings(fixtures: &[TextFixture], expected: bool) {
    for (path, text) in fixtures {
        let source = source(path, text);
        let report = scan_source(&source);
        let rules = duplicate_rules(&report);
        if expected {
            assert!(
                !rules.is_empty(),
                "{path} should retain behavioral duplication findings"
            );
        } else {
            assert_eq!(
                rules,
                Vec::<&str>::new(),
                "{path} should not have structural duplication findings"
            );
        }
    }
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
fn inline_suppression_parses_same_line_rules() {
    let Some((next_line, rules)) = inline_ignore_rules_for_line(
        "x = 1  // deslop:ignore reimpl-empty? consecutive-blank-lines",
    ) else {
        panic!("expected inline suppression directive")
    };
    assert!(!next_line);
    assert_eq!(rules, vec!["reimpl-empty?", "consecutive-blank-lines"]);
}

#[test]
fn inline_suppression_parses_next_line_rules() {
    let Some((next_line, rules)) = inline_ignore_rules_for_line(
        "  # deslop:ignore-next-line  reimpl-empty?  -- keep source clean",
    ) else {
        panic!("expected inline suppression directive")
    };
    assert!(next_line);
    assert_eq!(rules, vec!["reimpl-empty?"]);
}

#[test]
fn inline_suppression_skips_reporting_when_directive_is_next_line() {
    let source = clojure_source(
        "(defn ok [x] x)\n; deslop:ignore-next-line reimpl-empty?\n(= (count ys) 0)\n",
    );
    let report = scan_source_with_config(
        &source,
        AnalyzerConfig {
            min_duplication_tokens: 0,
            ..AnalyzerConfig::default()
        },
    );
    assert!(!has_rule(&report, "reimpl-empty?"));
}

#[test]
fn inline_suppression_skips_reporting_when_directive_is_same_line() {
    let source = clojure_source("(= (count ys) 0) ; deslop:ignore reimpl-empty?\n");
    let report = scan_source_with_config(
        &source,
        AnalyzerConfig {
            min_duplication_tokens: 0,
            ..AnalyzerConfig::default()
        },
    );
    assert!(!has_rule(&report, "reimpl-empty?"));
}

#[test]
fn incompleteness_ignores_identifier_containing_placeholder() {
    // `placeholders` is a function name, not an unimplemented stub.
    let source = clojure_source("(defn- placeholders [coll]\n  (mapv (fn [_] \"?\") coll))\n");
    let report = scan_source(&source);
    assert!(!has_rule(&report, "incompleteness"));
}

#[test]
fn incompleteness_flags_standalone_placeholder() {
    let source = clojure_source("(defn compute [] placeholder)\n");
    let report = scan_source(&source);
    assert!(has_rule(&report, "incompleteness"));
}

#[test]
fn magic_number_flags_inline_literal() {
    let source = clojure_source("(defn area [r] (* r r 31415))\n");
    let report = scan_source(&source);
    assert!(has_rule(&report, "magic-number"));
}

#[test]
fn magic_number_skips_multiline_named_constant() {
    // Binding a literal to a name is the rule's own fix; flagging the value
    // line again would make the rule un-actionable.
    let source = clojure_source(
        "(def ^:private hnsw-edges-per-node\n  \"HNSW m: edges per node.\"\n  64)\n",
    );
    let report = scan_source(&source);
    assert!(!has_rule(&report, "magic-number"));
}

#[test]
fn magic_number_skips_numbers_in_docstring() {
    let source =
        clojure_source("(defn extract\n  \"Return 5-20 entities; supports 16 types.\"\n  [x] x)\n");
    let report = scan_source(&source);
    assert!(!has_rule(&report, "magic-number"));
}

#[test]
fn magic_number_skips_rust_multiline_const() {
    let fixture = source("constants.rs", "const MAX_BATCH: usize =\n    4096;\n");
    let report = scan_source(&fixture);
    assert!(!has_rule(&report, "magic-number"));
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
fn per_language_long_method_threshold_overrides_global() {
    let fixture = long_method_source(39);
    let report = scan_source_with_config(
        &fixture,
        AnalyzerConfig {
            long_method_nloc: 100,
            rust: AnalyzerLangConfig {
                long_method_nloc: Some(20),
            },
            ..AnalyzerConfig::default()
        },
    );
    assert!(has_rule(&report, "long-method"));
}

#[test]
fn fp_corpus_clean_structural_code_has_no_duplication_findings() {
    assert_duplication_findings(CLEAN_DUPLICATION_FIXTURES, false);
}

#[test]
fn tp_corpus_behavioral_duplicates_still_flagged() {
    assert_duplication_findings(BEHAVIORAL_DUPLICATION_FIXTURES, true);
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
fn julia_eachindex_suggestion_requires_same_collection_indexing() {
    let fixture = source(
        "sample.jl",
        "function f(xs)\n    for i in 1:length(xs)\n        println(xs[i])\n    end\nend\n",
    );
    let report = scan_source(&fixture);
    let finding = finding_for_rule(&report, "reimpl-eachindex");

    assert_eq!(finding.safety, SafetyClass::SafeWithPrecondition);
    assert!(finding.edit.is_none());
    assert!(finding.suggestion.contains("eachindex(xs)"));
    assert!(finding.suggestion.contains("ordinal counter"));
    assert_eq!(
        finding.precondition.as_deref(),
        Some("loop variable is used only to index the same collection")
    );
}

#[test]
fn julia_eachindex_suggestion_skips_ordinal_or_other_collection_use() {
    let fixture = source(
        "sample.jl",
        "function ordinal(xs)\n    for i in 1:length(xs)\n        println(i)\n    end\nend\n\nfunction other(xs, ys)\n    for i in 1:length(xs)\n        println(ys[i])\n    end\nend\n\nfunction mixed(xs)\n    for i in 1:length(xs)\n        println(xs[i], i)\n    end\nend\n",
    );
    let report = scan_source(&fixture);

    assert!(
        !has_rule(&report, "reimpl-eachindex"),
        "ordinal counters and other-collection indexes must not get eachindex suggestions"
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
fn python_idiom_pack_flags_seed_rules() {
    let fixture = source(
        "sample.py",
        "if value == None:\n    pass\nfor idx in range(len(items)):\n    print(items[idx])\nif key in data.keys():\n    pass\nvalues = list([x for x in items])\n",
    );
    let report = scan_source(&fixture);
    for rule in [
        "py-none-comparison",
        "py-range-len",
        "py-dict-keys-membership",
        "py-list-comprehension-wrapper",
    ] {
        assert!(has_rule(&report, rule), "missing {rule}");
    }
}

#[test]
fn javascript_and_typescript_packs_flag_seed_rules() {
    for path in ["sample.js", "sample.ts"] {
        let fixture = source(
            path,
            "var count = 0;\nif (count == null) {\n  count = 1;\n}\nasync function load() {\n  return await fetch('/x');\n}\n",
        );
        let report = scan_source(&fixture);
        for rule in [
            "js-var-declaration",
            "js-loose-equality",
            "js-unnecessary-await",
        ] {
            assert!(has_rule(&report, rule), "{path} missing {rule}");
        }
    }
}

#[test]
fn scan_paths_reports_cross_file_duplicates() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let left = tmp.path().join("left.rs");
    let right = tmp.path().join("right.rs");
    let body = "pub fn copied(xs: &[i32]) -> i32 {\n    let positive = xs.iter().filter(|x| **x > 0).count();\n    let adjusted = positive + 3;\n    if adjusted > 10 {\n        adjusted * 2\n    } else {\n        adjusted - 1\n    }\n}\n";
    std::fs::write(&left, body).expect("left");
    std::fs::write(&right, body.replace("copied", "pasted")).expect("right");
    let reports = scan_paths_with_config(
        &[tmp.path().to_path_buf()],
        AnalyzerConfig {
            min_duplication_tokens: 16,
            min_meaningful_tokens: 4,
            ..AnalyzerConfig::default()
        },
    )
    .expect("scan");
    assert!(
        reports
            .iter()
            .flat_map(|report| &report.findings)
            .any(|finding| finding.rule == "duplicate-block"
                && finding.message.contains("left.rs")),
        "expected cross-file duplicate-block finding: {reports:#?}"
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

fn blank_source(path: &str) -> SourceFile {
    source(path, "a = 1\n\n\nb = 2\n")
}

fn config_with(suppression: Suppression) -> AnalyzerConfig {
    AnalyzerConfig {
        suppression,
        ..AnalyzerConfig::default()
    }
}

#[test]
fn empty_suppression_is_a_noop() {
    assert!(Suppression::default().is_empty());
    let report = scan_source_with_config(&blank_source("sample.py"), AnalyzerConfig::default());
    assert!(has_rule(&report, "consecutive-blank-lines"));
}

#[test]
fn suppression_disables_rule_entirely() {
    let mut builder = Suppression::builder();
    builder.disable_rule("consecutive-blank-lines");
    let report = scan_source_with_config(
        &blank_source("sample.py"),
        config_with(builder.build().expect("build suppression")),
    );
    assert!(!has_rule(&report, "consecutive-blank-lines"));
}

#[test]
fn suppression_global_ignore_path_skips_matching_files_only() {
    let mut builder = Suppression::builder();
    builder.ignore_path("vendor/**");
    let suppression = builder.build().expect("build suppression");

    let ignored = scan_source_with_config(
        &blank_source("vendor/sample.py"),
        config_with(suppression.clone()),
    );
    assert!(!has_rule(&ignored, "consecutive-blank-lines"));

    let kept = scan_source_with_config(&blank_source("src/sample.py"), config_with(suppression));
    assert!(has_rule(&kept, "consecutive-blank-lines"));
}

#[test]
fn suppression_per_rule_ignore_path_is_scoped_to_that_rule() {
    let mut builder = Suppression::builder();
    builder.ignore_path_for_rule("consecutive-blank-lines", "ignored/**");
    let suppression = builder.build().expect("build suppression");

    let ignored = scan_source_with_config(
        &blank_source("ignored/sample.py"),
        config_with(suppression.clone()),
    );
    assert!(!has_rule(&ignored, "consecutive-blank-lines"));

    let kept = scan_source_with_config(&blank_source("src/sample.py"), config_with(suppression));
    assert!(has_rule(&kept, "consecutive-blank-lines"));
}

#[test]
fn suppression_rejects_unknown_rule_name() {
    let mut builder = Suppression::builder();
    builder.disable_rule("ignore-comments");
    let err = builder.build().expect_err("unknown rule must error");
    assert!(
        err.to_string().contains("unknown rule 'ignore-comments'"),
        "error should name the offending rule: {err}"
    );
}

#[test]
fn suppression_rejects_invalid_glob() {
    let mut builder = Suppression::builder();
    builder.ignore_path("a/[unterminated");
    assert!(builder.build().is_err());
}
