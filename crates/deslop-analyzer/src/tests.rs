use std::fs;
use std::path::{Path, PathBuf};

use deslop_core::{AnalysisStatus, Finding, Lang, SafetyClass};
use deslop_parse::{ProjectAnalysis, ProjectSnapshotBuilder, RepositoryId, SourceFile};

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

#[test]
fn malformed_typescript_is_quarantined_before_rules_or_edits() {
    let source = SourceFile::new(
        PathBuf::from("malformed.ts"),
        include_str!("../../../tests/fixtures/typescript/malformed.ts").to_string(),
    );

    let report = scan_source(&source);

    assert_eq!(report.analysis.status, AnalysisStatus::Partial);
    assert!(!report.analysis.diagnostics.is_empty());
    assert!(report.findings.is_empty());
}

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
fn python_behavioral_regions_enable_decorated_long_method_detection() {
    let fixture = source(
        "behavioral.py",
        include_str!("../../../tests/fixtures/python/behavioral.py"),
    );
    let report = scan_source_with_config(
        &fixture,
        AnalyzerConfig {
            min_duplication_tokens: 0,
            long_method_nloc: 3,
            ..AnalyzerConfig::default()
        },
    );

    assert!(report.findings.iter().any(|finding| {
        finding.rule == "long-method" && finding.span.start_line == 5 && finding.span.end_line == 7
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.rule == "long-method"
            && finding.span.start_line == 13
            && finding.span.end_line == 18
    }));
}

#[test]
fn python_behavioral_regions_enable_callable_duplication_detection() {
    let fixture = source(
        "duplicates.py",
        "def left(value):\n    result = value * 3\n    if result > 10:\n        return result - 1\n    return result + 1\n\ndef right(item):\n    output = item * 3\n    if output > 10:\n        return output - 1\n    return output + 1\n",
    );
    let report = scan_source_with_config(
        &fixture,
        AnalyzerConfig {
            min_duplication_tokens: 10,
            min_meaningful_tokens: 4,
            ..AnalyzerConfig::default()
        },
    );

    assert!(
        !duplicate_rules(&report).is_empty(),
        "Python callable bodies should participate in duplication analysis"
    );
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
    for path in ["sample.js", "sample.ts", "sample.tsx"] {
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
fn tsx_uses_typescript_threshold_configuration() {
    let config = AnalyzerConfig {
        typescript: AnalyzerLangConfig {
            long_method_nloc: Some(37),
        },
        ..AnalyzerConfig::default()
    };

    let tsx = source("sample.tsx", "const view: JSX.Element = <div />;\n");
    assert_eq!(tsx.lang, Lang::TypeScript);
    assert_eq!(config.long_method_nloc_for(tsx.lang), 37);
}

#[test]
fn typed_typescript_dialects_preserve_inline_suppression() {
    let cases = [
        (
            "sample.ts",
            include_str!("../../../tests/fixtures/typescript/typed.ts"),
            "function suppressed(value: number): number {\n  // deslop:ignore-next-line js-var-declaration\n  var copy: number = value;\n  return copy;\n}\n",
        ),
        (
            "sample.tsx",
            include_str!("../../../tests/fixtures/typescript/component.tsx"),
            "function Suppressed(value: string): JSX.Element {\n  // deslop:ignore-next-line js-var-declaration\n  var copy: JSX.Element = <span>{value}</span>;\n  return copy;\n}\n",
        ),
    ];

    for (path, fixture, suppressed) in cases {
        let text = format!("{fixture}\n{suppressed}");
        let report = scan_source(&source(path, &text));
        assert_eq!(report.lang, Lang::TypeScript);
        assert!(!has_rule(&report, "js-var-declaration"), "{path}");
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
    assert!(julia_external_analyzer(&AnalyzerConfig::default()).is_none());
    assert!(
        julia_external_analyzer(&AnalyzerConfig {
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
    assert!(
        !has_rule(&ignored, "consecutive-blank-lines"),
        "{ignored:#?}"
    );

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
    assert!(
        !has_rule(&ignored, "consecutive-blank-lines"),
        "{ignored:#?}"
    );

    let kept = scan_source_with_config(&blank_source("src/sample.py"), config_with(suppression));
    assert!(has_rule(&kept, "consecutive-blank-lines"));
}

#[test]
fn effective_analyzer_snapshot_round_trips_suppression_and_boundary() {
    let mut builder = Suppression::builder();
    builder
        .disable_rule("magic-number")
        .ignore_path("vendor/**")
        .ignore_path_for_rule("long-method", "generated/**");
    let mut config = config_with(builder.build().expect("suppression"));
    config.long_method_nloc = 17;
    config.typescript.long_method_nloc = Some(23);
    config.boundary.extra_sinks = vec!["trace".to_string()];
    let snapshot = config.snapshot();
    let rebuilt = snapshot.to_config().expect("rebuild config");

    assert_eq!(rebuilt.snapshot(), snapshot);
    assert_eq!(snapshot.suppression.disabled_rules, ["magic-number"]);
    assert_eq!(snapshot.suppression.ignore_paths, ["vendor/**"]);
    assert_eq!(snapshot.suppression.rules["long-method"], ["generated/**"]);
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

#[test]
fn scan_paths_deduplicates_repeated_and_overlapping_inputs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source_path = temp.path().join("sample.rs");
    fs::write(&source_path, "fn unfinished() {\n    todo!();\n}\n").expect("write source");

    let reports = scan_paths(&[
        source_path.clone(),
        temp.path().to_path_buf(),
        source_path.clone(),
    ])
    .expect("scan overlapping inputs");

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].path, source_path);
}

#[test]
fn path_deduplication_is_input_order_invariant_and_prefers_relative_paths() {
    let cwd = std::env::current_dir().expect("current directory");
    let temp = tempfile::tempdir_in(&cwd).expect("tempdir in current directory");
    let absolute = temp.path().join("sample.rs");
    fs::write(&absolute, "fn sample() {}\n").expect("write source");
    let relative = absolute
        .strip_prefix(&cwd)
        .expect("relative fixture path")
        .to_path_buf();
    let dotted = PathBuf::from(".").join(&relative);

    let forward = scan_paths(&[absolute.clone(), dotted.clone(), relative.clone()]).unwrap();
    let reversed = scan_paths(&[relative.clone(), absolute, dotted]).unwrap();

    assert_eq!(forward.len(), 1);
    assert_eq!(reversed.len(), 1);
    assert_eq!(forward[0].path, relative);
    assert_eq!(forward[0].path, reversed[0].path);
}

#[test]
fn per_file_candidate_cache_reuses_exact_repeat_and_only_misses_changed_successor() {
    let root = tempfile::tempdir().unwrap();
    let cache_root = tempfile::tempdir().unwrap();
    let cache = PersistentArtifactCache::open(cache_root.path()).unwrap();
    let repository = RepositoryId::explicit("analyzer-incremental-cache").unwrap();
    let build = |left: &[u8]| {
        ProjectSnapshotBuilder::new(root.path(), repository.clone())
            .unwrap()
            .with_overlay("src/a.rs", left.to_vec())
            .unwrap()
            .with_overlay("src/b.rs", b"fn stable() -> i32 { 2 }\n".to_vec())
            .unwrap()
            .build()
            .unwrap()
    };
    let mut config = AnalyzerConfig::default();
    config.boundary.enabled = false;
    let analysis = ProjectAnalysis::build(build(b"fn value() -> i32 { 1 }\n")).unwrap();

    let cold = scan_analysis_with_cache(analysis.clone(), config.clone(), cache.clone()).unwrap();
    assert_eq!((cold.local_cache_hits, cold.local_cache_misses), (0, 2));
    let repeated =
        scan_analysis_with_cache(analysis.clone(), config.clone(), cache.clone()).unwrap();
    assert_eq!(
        (repeated.local_cache_hits, repeated.local_cache_misses),
        (2, 0)
    );
    assert_eq!(cold.local_commit_id, repeated.local_commit_id);
    assert_eq!(
        serde_json::to_value(&cold.reports).unwrap(),
        serde_json::to_value(&repeated.reports).unwrap()
    );

    let update = analysis
        .successor(build(b"fn value() -> i32 { 3 }\n"))
        .unwrap();
    assert_eq!(update.instrumentation().incremental_files, 1);
    assert_eq!(update.instrumentation().reused_files, 1);
    let successor = scan_analysis_with_cache(update.into_current(), config, cache).unwrap();
    assert_eq!(
        (successor.local_cache_hits, successor.local_cache_misses),
        (1, 1)
    );
}

#[test]
fn prepared_projection_identity_binds_presentation_paths() {
    let root = tempfile::tempdir().expect("tempdir");
    let snapshot = ProjectSnapshotBuilder::new(
        root.path(),
        RepositoryId::explicit("prepared-presentation").unwrap(),
    )
    .unwrap()
    .with_overlay("sample.rs", b"fn unfinished() { todo!(); }\n".to_vec())
    .unwrap()
    .build()
    .unwrap();
    let analysis = ProjectAnalysis::build(snapshot).unwrap();
    let manifest = AnalyzerInputManifest {
        report_sources: vec![PathBuf::from("sample.rs")],
        boundary_artifacts: Vec::new(),
        boundary_coverage: BoundaryCoverage::Unavailable {
            reason: "disabled".to_string(),
        },
        external_unavailable_reason: "not prepared".to_string(),
    };
    let first = PreparedAnalyzerAnalysis::new(
        analysis.clone(),
        manifest.clone(),
        SnapshotPresentationMap::from_entries([(
            PathBuf::from("sample.rs"),
            PathBuf::from("first.rs"),
        )])
        .unwrap(),
    )
    .unwrap();
    let second = PreparedAnalyzerAnalysis::new(
        analysis,
        manifest,
        SnapshotPresentationMap::from_entries([(
            PathBuf::from("sample.rs"),
            PathBuf::from("second.rs"),
        )])
        .unwrap(),
    )
    .unwrap();
    let mut config = AnalyzerConfig::default();
    config.boundary.enabled = false;
    let first = scan_prepared_analysis(first, config.clone()).unwrap();
    let second = scan_prepared_analysis(second, config).unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(first.reports[0].path, Path::new("first.rs"));
    assert_eq!(second.reports[0].path, Path::new("second.rs"));
    assert_ne!(
        first.reports[0].findings[0].fingerprint,
        second.reports[0].findings[0].fingerprint
    );
}

#[test]
fn prepared_boundary_withholds_project_claims_when_any_source_is_partial() {
    let root = tempfile::tempdir().expect("tempdir");
    fs::write(root.path().join("settings.toml"), "used_knob = 1\n").unwrap();
    fs::write(
        root.path().join("broken.ts"),
        "const used_knob = config.get('used-knob');\nfunction broken( {\n",
    )
    .unwrap();
    deslop_parse::reset_parse_source_invocations();
    let reports = scan_paths(&[root.path().to_path_buf()]).unwrap();
    assert!(
        reports
            .iter()
            .any(|report| report.analysis.status == AnalysisStatus::Partial)
    );
    assert!(
        reports
            .iter()
            .flat_map(|report| &report.findings)
            .all(|finding| !finding.rule.starts_with("config-key-"))
    );
    assert_eq!(deslop_parse::parse_source_invocations(), 0);
}

#[test]
fn prepared_boundary_is_revision_pinned_parse_once_and_deterministic() {
    let root = tempfile::tempdir().expect("tempdir");
    let artifact = root.path().join("settings.toml");
    let code = root.path().join("main.jl");
    fs::write(
        &artifact,
        "phantom_knob = 1\necho_only = 2\nshadowed_knob = 3\nlive_knob = 4\n",
    )
    .unwrap();
    fs::write(
        &code,
        concat!(
            "echo_only = parse(Int, get(options, \"echo-only\", \"2\"))\n",
            "println(echo_only)\n",
            "shadowed_knob = parse(Int, get(options, \"shadowed-knob\", \"3\"))\n",
            "shadowed_knob = min(shadowed_knob, 3)\n",
            "run_shadowed(shadowed_knob)\n",
            "live_knob = parse(Int, get(options, \"live-knob\", \"4\"))\n",
            "run_live(live_knob)\n",
        ),
    )
    .unwrap();

    let build_prepared = || {
        let mut planner = ProjectSnapshotPlanner::resolve(ProjectSnapshotRequest {
            invocation_base: root.path().to_path_buf(),
            root: RootSpec::Explicit(root.path().to_path_buf()),
            repository: RepositorySpec::Explicit(
                RepositoryId::explicit("prepared-boundary-pinning").unwrap(),
            ),
            scope: ScopeSpec::Requested(vec![PathBuf::from(".")]),
            discovery: DiscoveryPolicy::LegacyRespectIgnore,
        })
        .unwrap();
        let boundary_artifact = planner.add_disk_analysis_input(&artifact).unwrap();
        let built = planner.build().unwrap();
        let analysis = ProjectAnalysis::build(built.snapshot).unwrap();
        let manifest = AnalyzerInputManifest {
            report_sources: analysis
                .files()
                .map(|file| file.key().path.clone())
                .collect(),
            boundary_artifacts: vec![boundary_artifact],
            boundary_coverage: BoundaryCoverage::Complete,
            external_unavailable_reason: "not prepared".to_string(),
        };
        PreparedAnalyzerAnalysis::new(analysis, manifest, built.presentation).unwrap()
    };

    deslop_parse::reset_parse_source_invocations();
    let prepared = build_prepared();
    let cold_counts = prepared.analysis.parse_counts();
    assert_eq!(cold_counts.len(), 1);
    assert!(cold_counts.values().all(|count| {
        (
            count.requested,
            count.owners,
            count.parser_invocations,
            count.reused,
        ) == (1, 1, 1, 0)
    }));
    let first = scan_prepared_analysis(prepared.clone(), AnalyzerConfig::default()).unwrap();
    let second = scan_prepared_analysis(prepared.clone(), AnalyzerConfig::default()).unwrap();
    let boundary = first
        .reports
        .iter()
        .flat_map(|report| &report.findings)
        .filter(|finding| finding.rule.starts_with("config-key-"))
        .collect::<Vec<_>>();
    assert_eq!(
        boundary
            .iter()
            .map(|finding| finding.rule.as_str())
            .collect::<Vec<_>>(),
        [
            "config-key-unconsumed",
            "config-key-shadowed",
            "config-key-unread"
        ]
    );
    assert_eq!(first.id, second.id);
    assert_eq!(
        serde_json::to_string(&first.reports).unwrap(),
        serde_json::to_string(&second.reports).unwrap()
    );

    fs::write(&artifact, "replacement_key = 9\n").unwrap();
    fs::write(&code, "run_replacement()\n").unwrap();
    let pinned = scan_prepared_analysis(prepared, AnalyzerConfig::default()).unwrap();
    assert_eq!(first.id, pinned.id);
    assert_eq!(
        serde_json::to_string(&first.reports).unwrap(),
        serde_json::to_string(&pinned.reports).unwrap()
    );
    assert_eq!(first.analysis.parse_counts(), cold_counts);
    assert_eq!(deslop_parse::parse_source_invocations(), 0);

    let rebuilt = scan_prepared_analysis(build_prepared(), AnalyzerConfig::default()).unwrap();
    assert_ne!(first.id, rebuilt.id);
}

#[test]
fn invalid_utf8_boundary_artifact_downgrades_without_negative_claims() {
    let root = tempfile::tempdir().expect("tempdir");
    fs::write(root.path().join("settings.toml"), [0xff, b'=', b'1', b'\n']).unwrap();
    fs::write(root.path().join("main.jl"), "run()\n").unwrap();
    let reports = scan_paths(&[root.path().to_path_buf()]).unwrap();
    let artifact = reports
        .iter()
        .find(|report| report.path.ends_with("settings.toml"))
        .expect("invalid boundary artifact has a report");
    assert_eq!(artifact.analysis.status, AnalysisStatus::Failed);
    assert_eq!(
        artifact.analysis.diagnostics[0].code,
        "invalid-utf8-analysis-input"
    );
    assert!(
        reports
            .iter()
            .flat_map(|report| &report.findings)
            .all(|finding| !finding.rule.starts_with("config-key-"))
    );
}

#[test]
fn primary_analyzer_and_metrics_surfaces_have_static_ownership_guards() {
    fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let start = source.find(start).expect("guard start marker");
        let end = source[start..].find(end).expect("guard end marker") + start;
        &source[start..end]
    }

    let analyzer = include_str!("lib.rs");
    let view = between(analyzer, "pub struct AnalyzerFile", "impl Suppression");
    let metrics = include_str!("../../deslop-metrics/src/lib.rs");
    let metrics = between(metrics, "use std::collections", "#[cfg(test)]");

    for (name, source) in [
        ("analyzer", analyzer),
        ("agnostic analyzer", include_str!("agnostic.rs")),
        ("boundary analyzer", include_str!("boundary.rs")),
        ("clojure analyzer", include_str!("clojure.rs")),
        ("julia analyzer", include_str!("julia.rs")),
        ("javascript analyzer", include_str!("packs/javascript.rs")),
        ("python analyzer", include_str!("packs/python.rs")),
        ("rust analyzer", include_str!("packs/rust.rs")),
        ("token analyzer", include_str!("tokens.rs")),
        ("metrics", metrics),
    ] {
        for forbidden in [
            "parse_source",
            "SourceFile::read",
            "read_to_string",
            "pack_for_path",
            "supported_pack_for_path",
            "pack_for_lang",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} reintroduced forbidden snapshot-bypass operation {forbidden}"
            );
        }
    }
    assert!(!view.contains("source: SourceFile"));
    assert!(!view.contains("-> &SourceFile"));
}

#[test]
fn source_compatibility_adapter_is_snapshot_owned_and_deterministic() {
    let source = source("compat.rs", "fn answer() -> i32 { 42 }\n");
    deslop_parse::reset_parse_source_invocations();

    let first = scan_source(&source);
    let second = scan_source(&source);

    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&second).unwrap()
    );
    assert_eq!(deslop_parse::parse_source_invocations(), 0);
}

#[test]
fn owned_scan_analysis_is_parse_once_deterministic_and_partial_safe() {
    let root = tempfile::tempdir().expect("tempdir");
    deslop_parse::reset_parse_source_invocations();
    let snapshot = ProjectSnapshotBuilder::new(
        root.path(),
        RepositoryId::explicit("owned-analyzer-matrix").unwrap(),
    )
    .unwrap()
    .with_overlay(
        "tests/fixtures/python/behavioral.py",
        include_bytes!("../../../tests/fixtures/python/behavioral.py").to_vec(),
    )
    .unwrap()
    .with_overlay(
        "tests/fixtures/dup/behavior.rs",
        include_bytes!("../../../tests/fixtures/dup/behavior.rs").to_vec(),
    )
    .unwrap()
    .with_overlay(
        "tests/fixtures/typescript/component.tsx",
        include_bytes!("../../../tests/fixtures/typescript/component.tsx").to_vec(),
    )
    .unwrap()
    .with_overlay(
        "tests/fixtures/typescript/malformed.ts",
        include_bytes!("../../../tests/fixtures/typescript/malformed.ts").to_vec(),
    )
    .unwrap()
    .with_overlay(
        "idioms.rs",
        b"fn foo(x: i32) -> i32 { x }\nfn f(v: Vec<String>, xs: Vec<i32>) -> Vec<i32> {\n    let _ = &v.clone();\n    xs.into_iter().map(|x| foo(x)).collect()\n}\nfn tail() -> i32 {\n    return 1;\n}\n"
            .to_vec(),
    )
    .unwrap()
    .build()
    .unwrap();
    let analysis = ProjectAnalysis::build(snapshot).unwrap();
    let cold_counts = analysis.parse_counts();
    assert_eq!(cold_counts.len(), 5);
    assert!(cold_counts.values().all(|count| {
        (
            count.requested,
            count.owners,
            count.parser_invocations,
            count.reused,
        ) == (1, 1, 1, 0)
    }));
    let boundary_error = scan_analysis(analysis.clone(), AnalyzerConfig::default())
        .expect_err("source-only analysis must not claim boundary coverage");
    assert!(
        boundary_error
            .to_string()
            .contains("cannot prove config-boundary coverage")
    );
    assert_eq!(analysis.parse_counts(), cold_counts);
    assert_eq!(deslop_parse::parse_source_invocations(), 0);

    let mut config = AnalyzerConfig::default();
    config.boundary.enabled = false;
    config.python.long_method_nloc = Some(3);
    let first = scan_analysis(analysis.clone(), config.clone()).unwrap();
    let second = scan_analysis(analysis.clone(), config.clone()).unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(
        serde_json::to_string(&(
            &first.reports,
            &first.input_contents,
            &first.external_capabilities,
        ))
        .unwrap(),
        serde_json::to_string(&(
            &second.reports,
            &second.input_contents,
            &second.external_capabilities,
        ))
        .unwrap()
    );
    assert_eq!(analysis.parse_counts(), cold_counts);
    assert_eq!(deslop_parse::parse_source_invocations(), 0);

    let findings = first
        .reports
        .iter()
        .flat_map(|report| report.findings.iter())
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 9, "{findings:#?}");
    let expected = [
        ("consecutive-blank-lines", 2, 3, "6a501adaa439fdee"),
        ("long-method", 4, 9, "5497c8ef53d9cf57"),
        ("long-method", 5, 7, "2dddbe7c1175c1ea"),
        ("consecutive-blank-lines", 10, 11, "ddf3aa860042ca2e"),
        ("long-method", 13, 18, "a63349b9dc2deef7"),
        ("near-duplicate", 13, 18, "e78e22cf018ba4cd"),
        ("needless-clone", 3, 3, "22249ca9c4db7d7f"),
        ("redundant-closure", 4, 4, "dd9a9bdf11c2f805"),
        ("needless-return", 7, 7, "ca78faf42b98702e"),
    ];
    for (rule, start, end, fingerprint) in expected {
        assert!(
            findings.iter().any(|finding| {
                finding.rule == rule
                    && finding.span.start_line == start
                    && finding.span.end_line == end
                    && finding.fingerprint == fingerprint
            }),
            "missing {rule} {start}..{end} {fingerprint}: {findings:#?}"
        );
    }
    let malformed = first
        .reports
        .iter()
        .find(|report| report.path.ends_with("malformed.ts"))
        .unwrap();
    assert_eq!(malformed.analysis.status, AnalysisStatus::Partial);
    assert_eq!(malformed.findings.len(), 0);
    assert!(malformed.analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "tree-sitter-error"
            && diagnostic.span.is_some_and(|span| {
                span.start_line == 2 && span.start_byte == 62 && span.end_byte == 63
            })
    }));

    config.python.long_method_nloc = Some(4);
    let threshold_four = scan_analysis(analysis.clone(), config).unwrap();
    assert_ne!(first.id, threshold_four.id);
    assert_eq!(
        threshold_four
            .reports
            .iter()
            .map(|report| report.findings.len())
            .sum::<usize>(),
        8
    );
    assert_eq!(analysis.parse_counts(), cold_counts);
    assert_eq!(deslop_parse::parse_source_invocations(), 0);
}

#[test]
fn owned_scan_masks_strings_comments_and_constants_but_keeps_real_rust_evidence() {
    let root = tempfile::tempdir().unwrap();
    let source = r#"const LIMIT: i32 = 4096;
fn unfinished() -> i32 {
    let text = "TODO: implement 77";
    // placeholder 88
    todo!();
    let marker = "deslop:ignore-next-line needless-return";
    return 31415;
}
fn suppressed() -> i32 {
    // deslop:ignore-next-line needless-return
    return 2718;
}
"#;
    let snapshot = ProjectSnapshotBuilder::new(
        root.path(),
        RepositoryId::explicit("owned-rust-mask-test").unwrap(),
    )
    .unwrap()
    .with_overlay("masked.rs", source.as_bytes().to_vec())
    .unwrap()
    .build()
    .unwrap();
    let analysis = ProjectAnalysis::build(snapshot).unwrap();
    let counts = analysis.parse_counts();
    deslop_parse::reset_parse_source_invocations();
    let mut config = AnalyzerConfig::default();
    config.boundary.enabled = false;
    config.min_duplication_tokens = 0;
    let projection = scan_analysis(analysis.clone(), config).unwrap();
    let findings = &projection.reports[0].findings;
    assert_eq!(
        findings
            .iter()
            .map(|finding| (finding.rule.as_str(), finding.span.start_line))
            .collect::<Vec<_>>(),
        [
            ("incompleteness", 5),
            ("magic-number", 7),
            ("needless-return", 7),
        ]
    );
    assert_eq!(analysis.parse_counts(), counts);
    assert_eq!(deslop_parse::parse_source_invocations(), 0);
}

#[test]
fn owned_scan_computes_cross_file_duplication_without_reparse() {
    let root = tempfile::tempdir().unwrap();
    let first =
        b"fn alpha(input: i32) -> i32 {\n    let value = input + 10;\n    value * value - 3\n}\n";
    let second =
        b"fn beta(item: i32) -> i32 {\n    let result = item + 10;\n    result * result - 3\n}\n";
    let snapshot = ProjectSnapshotBuilder::new(
        root.path(),
        RepositoryId::explicit("owned-cross-file-test").unwrap(),
    )
    .unwrap()
    .with_overlay("a.rs", first.to_vec())
    .unwrap()
    .with_overlay("b.rs", second.to_vec())
    .unwrap()
    .build()
    .unwrap();
    let analysis = ProjectAnalysis::build(snapshot).unwrap();
    let counts = analysis.parse_counts();
    deslop_parse::reset_parse_source_invocations();
    let mut config = AnalyzerConfig::default();
    config.boundary.enabled = false;
    config.min_duplication_tokens = 12;
    config.min_meaningful_tokens = 5;
    let projection = scan_analysis(analysis.clone(), config).unwrap();
    let cross_file = projection
        .reports
        .iter()
        .flat_map(|report| report.findings.iter())
        .filter(|finding| matches!(finding.rule.as_str(), "duplicate-block" | "near-duplicate"))
        .collect::<Vec<_>>();
    assert!(!cross_file.is_empty(), "{:#?}", projection.reports);
    assert!(
        cross_file
            .iter()
            .any(|finding| finding.path == Path::new("b.rs") && finding.message.contains("a.rs:"))
    );
    assert_eq!(analysis.parse_counts(), counts);
    assert_eq!(deslop_parse::parse_source_invocations(), 0);
}

#[test]
fn owned_scan_ignores_legacy_suppression_match_root_in_identity_and_results() {
    let root = tempfile::tempdir().unwrap();
    let snapshot = ProjectSnapshotBuilder::new(
        root.path(),
        RepositoryId::explicit("owned-suppression-path-test").unwrap(),
    )
    .unwrap()
    .with_overlay("vendor/sample.rs", b"fn sample() {}\n\n\n".to_vec())
    .unwrap()
    .build()
    .unwrap();
    let analysis = ProjectAnalysis::build(snapshot).unwrap();
    let mut builder = Suppression::builder();
    builder.ignore_path("sample.rs");
    let suppression = builder.build().unwrap();
    let mut canonical = AnalyzerConfig::default();
    canonical.boundary.enabled = false;
    canonical.min_duplication_tokens = 0;
    canonical.suppression = suppression.clone();
    let mut rooted = canonical.clone();
    rooted.suppression = suppression.with_match_root(PathBuf::from("vendor"));

    let canonical = scan_analysis(analysis.clone(), canonical).unwrap();
    let rooted = scan_analysis(analysis, rooted).unwrap();
    assert_eq!(canonical.id, rooted.id);
    assert_eq!(
        serde_json::to_value(&canonical.reports).unwrap(),
        serde_json::to_value(&rooted.reports).unwrap()
    );
    assert!(has_rule(&canonical.reports[0], "consecutive-blank-lines"));
}

#[test]
fn owned_scan_dispatches_from_each_exact_stored_adapter() {
    let root = tempfile::tempdir().unwrap();
    let snapshot = ProjectSnapshotBuilder::new(
        root.path(),
        RepositoryId::explicit("owned-pack-dispatch-test").unwrap(),
    )
    .unwrap()
    .with_overlay(
        "sample.py",
        b"if value == None:\n    pass\nfor idx in range(len(items)):\n    print(items[idx])\nif key in data.keys():\n    pass\nvalues = list([x for x in items])\n".to_vec(),
    )
    .unwrap()
    .with_overlay(
        "sample.js",
        b"var count = 0;\nif (count == null) {\n  count = 1;\n}\nasync function load() {\n  return await fetch('/x');\n}\n".to_vec(),
    )
    .unwrap()
    .with_overlay(
        "sample.tsx",
        b"var count: number = 0;\nif (count == null) { count = 1; }\nasync function load(): Promise<void> { return await fetch('/x'); }\n".to_vec(),
    )
    .unwrap()
    .with_overlay(
        "sample.clj",
        b"(def a (not (= x y)))\n(def b (not (nil? z)))\n(def c (if p true false))\n".to_vec(),
    )
    .unwrap()
    .with_overlay(
        "sample.jl",
        b"function f(xs)\n    for i in 1:length(xs)\n        println(xs[i])\n    end\nend\n".to_vec(),
    )
    .unwrap()
    .build()
    .unwrap();
    let analysis = ProjectAnalysis::build(snapshot).unwrap();
    for (path, adapter) in [
        ("sample.py", "python"),
        ("sample.js", "javascript"),
        ("sample.tsx", "typescript"),
        ("sample.clj", "clojure"),
        ("sample.jl", "julia"),
    ] {
        assert_eq!(
            analysis.language_adapter(Path::new(path)).unwrap().name(),
            adapter
        );
    }
    let mut config = AnalyzerConfig::default();
    config.boundary.enabled = false;
    config.min_duplication_tokens = 0;
    let projection = scan_analysis(analysis, config).unwrap();
    assert_eq!(projection.external_capabilities.len(), 1);
    assert_eq!(projection.external_capabilities[0].analyzer, "clj-kondo");
    assert!(!projection.external_capabilities[0].available);
    assert_eq!(
        projection.external_capabilities[0].covered_rules,
        [
            "unused-binding",
            "unused-private-def",
            "unused-namespace",
            "redundant-do",
        ]
    );
    let rules = |path: &str| {
        projection
            .reports
            .iter()
            .find(|report| report.path == Path::new(path))
            .unwrap()
            .findings
            .iter()
            .map(|finding| finding.rule.as_str())
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        rules("sample.py"),
        [
            "py-dict-keys-membership",
            "py-list-comprehension-wrapper",
            "py-none-comparison",
            "py-range-len",
        ]
        .into_iter()
        .collect()
    );
    for path in ["sample.js", "sample.tsx"] {
        assert_eq!(
            rules(path),
            [
                "js-loose-equality",
                "js-unnecessary-await",
                "js-var-declaration",
            ]
            .into_iter()
            .collect()
        );
    }
    assert_eq!(
        rules("sample.clj"),
        ["reimpl-boolean", "reimpl-not=", "reimpl-some?"]
            .into_iter()
            .collect()
    );
    assert_eq!(
        rules("sample.jl"),
        ["reimpl-eachindex"].into_iter().collect()
    );
}
