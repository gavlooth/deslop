use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use deslop_analyzer::{AnalyzerConfig, scan_analysis};
use deslop_core::{AnalysisStatus, SafetyClass};
use deslop_graph::{GraphConfidence, GraphConfig, GraphEdgeKind, graph_analysis};
use deslop_metrics::{MetricsConfig, metrics_analysis};
use deslop_parse::{
    AdapterCapability, CANONICAL_ROLE_PROJECTION_SCHEMA, CONSTRUCT_POLICY_PROJECTION_SCHEMA,
    CanonicalRoleSet, CapabilityAuthority, CapabilitySupport, ConstructPolicyFactKind,
    ConstructPolicyKind, LANGUAGE_QUERY_PROJECTION_SCHEMA, LEXICAL_TOKEN_PROJECTION_SCHEMA, NodeId,
    ProjectAnalysis, ProjectSnapshotBuilder, QueryFamily, RawSyntaxFact, RepositoryId,
    SemanticTier,
};

#[test]
fn m2_adapter_definition_of_done_joins_every_fact_to_exact_authority() {
    struct AdapterRow {
        path: &'static str,
        dialect: &'static str,
        grammar_id: &'static str,
        grammar_version: &'static str,
    }

    let rows = [
        AdapterRow {
            path: "adapter_matrix.rs",
            dialect: "rust",
            grammar_id: "tree-sitter-rust",
            grammar_version: "0.24.2",
        },
        AdapterRow {
            path: "adapter_matrix.js",
            dialect: "javascript",
            grammar_id: "tree-sitter-javascript",
            grammar_version: "0.25.0",
        },
        AdapterRow {
            path: "adapter_matrix.ts",
            dialect: "typescript",
            grammar_id: "tree-sitter-typescript/typescript",
            grammar_version: "0.23.2",
        },
        AdapterRow {
            path: "adapter_matrix.tsx",
            dialect: "tsx",
            grammar_id: "tree-sitter-typescript/tsx",
            grammar_version: "0.23.2",
        },
        AdapterRow {
            path: "adapter_matrix.py",
            dialect: "python",
            grammar_id: "tree-sitter-python",
            grammar_version: "0.25.0",
        },
        AdapterRow {
            path: "adapter_matrix.clj",
            dialect: "clojure",
            grammar_id: "tree-sitter-clojure",
            grammar_version: "0.1.0",
        },
        AdapterRow {
            path: "adapter_matrix.jl",
            dialect: "julia",
            grammar_id: "tree-sitter-julia",
            grammar_version: "0.23.1",
        },
    ];
    let overlays: [(&str, &[u8]); 7] = [
        (
            "adapter_matrix.rs",
            include_bytes!("../../../tests/fixtures/rust/adapter_matrix.rs"),
        ),
        (
            "adapter_matrix.js",
            include_bytes!("../../../tests/fixtures/typescript/adapter_matrix.js"),
        ),
        (
            "adapter_matrix.ts",
            include_bytes!("../../../tests/fixtures/typescript/adapter_matrix.ts"),
        ),
        (
            "adapter_matrix.tsx",
            include_bytes!("../../../tests/fixtures/typescript/adapter_matrix.tsx"),
        ),
        (
            "adapter_matrix.py",
            include_bytes!("../../../tests/fixtures/python/adapter_matrix.py"),
        ),
        (
            "adapter_matrix.clj",
            include_bytes!("../../../tests/fixtures/clojure/adapter_matrix.clj"),
        ),
        (
            "adapter_matrix.jl",
            include_bytes!("../../../tests/fixtures/julia/adapter_matrix.jl"),
        ),
    ];
    let root = tempfile::tempdir().expect("M2 gold root");
    let mut builder = ProjectSnapshotBuilder::new(
        root.path(),
        RepositoryId::explicit("m2-adapter-definition-of-done").unwrap(),
    )
    .unwrap();
    for (path, source) in overlays {
        builder = builder
            .with_overlay(path, source.to_vec())
            .expect("M2 gold overlay");
    }
    let analysis = ProjectAnalysis::build(builder.build().unwrap()).unwrap();
    let parse_counts = analysis.parse_counts();
    assert_eq!(parse_counts.len(), 7);
    assert!(
        parse_counts
            .values()
            .all(|count| count.parser_invocations == 1)
    );

    let mut role_facts = 0;
    let mut role_assignments = 0;
    let mut lexical_facts = 0;
    let mut construct_facts = 0;
    let mut query_captures = 0;

    for row in rows {
        let path = Path::new(row.path);
        let entry = analysis.snapshot().entry(path).expect("M2 gold entry");
        let grammar = entry.grammar().expect("M2 gold grammar");
        let identity = entry
            .language_adapter_identity()
            .expect("M2 gold adapter identity");

        assert_eq!(grammar.dialect(), row.dialect);
        assert_eq!(grammar.grammar_id(), row.grammar_id);
        assert_eq!(grammar.grammar_version(), row.grammar_version);
        assert!(!grammar.selector().is_empty());
        assert!(!grammar.parser_build().is_empty());
        assert_eq!(identity.schema(), "deslop-lang-adapter/3");
        assert_eq!(identity.capabilities().adapter_schema(), identity.schema());
        assert_eq!(identity.queries().adapter_schema(), identity.schema());
        assert_eq!(
            identity.lexical_policy().adapter_schema(),
            identity.schema()
        );
        assert_eq!(
            identity.construct_policy().adapter_schema(),
            identity.schema()
        );
        assert_eq!(
            identity.capabilities().highest_complete_tier(),
            Some(SemanticTier::S1)
        );
        for capability in AdapterCapability::ALL {
            let declaration = identity.capabilities().declaration(capability);
            let expected = if capability.tier() <= SemanticTier::S1
                || (row.dialect == "rust" && capability == AdapterCapability::ControlFlow)
            {
                CapabilitySupport::Provided
            } else {
                CapabilitySupport::Unknown
            };
            assert_eq!(declaration.support(), expected);
            assert_eq!(
                declaration.authority().is_some(),
                expected == CapabilitySupport::Provided
            );
        }

        let roles = analysis
            .canonical_role_projection(path)
            .expect("M2 role projection");
        assert_eq!(roles.schema(), CANONICAL_ROLE_PROJECTION_SCHEMA);
        assert!(roles.schema().ends_with("/1"));
        assert!(!roles.id().as_str().is_empty());
        assert!(Arc::ptr_eq(roles.analysis(), &analysis));
        assert_eq!(roles.path(), path);
        let role_capability = identity
            .capabilities()
            .declaration(AdapterCapability::CanonicalRoles);
        assert_eq!(role_capability.support(), CapabilitySupport::Provided);
        assert_eq!(
            role_capability.authority(),
            Some(CapabilityAuthority::Adapter)
        );
        let mut roles_by_node = BTreeMap::<NodeId, CanonicalRoleSet>::new();
        for fact in roles.facts() {
            assert_raw_fact(&analysis, path, fact.node(), fact.raw());
            assert!(roles_by_node.insert(fact.node(), fact.roles()).is_none());
            role_facts += 1;
            role_assignments += fact.roles().len();
        }

        let lexical = analysis
            .lexical_token_projection(path)
            .expect("M2 lexical projection");
        assert_eq!(lexical.schema(), LEXICAL_TOKEN_PROJECTION_SCHEMA);
        assert!(lexical.schema().ends_with("/1"));
        assert!(!lexical.id().as_str().is_empty());
        assert!(Arc::ptr_eq(lexical.analysis(), &analysis));
        assert_eq!(lexical.path(), path);
        assert_eq!(lexical.policy(), identity.lexical_policy());
        assert_eq!(lexical.policy().support(), CapabilitySupport::Provided);
        assert_eq!(
            lexical.policy().authority(),
            Some(CapabilityAuthority::Adapter)
        );
        for fact in lexical.facts() {
            assert_raw_fact(&analysis, path, fact.node(), fact.raw());
            assert!(!fact.text().is_empty());
            lexical_facts += 1;
        }

        let constructs = analysis
            .construct_policy_projection(path)
            .expect("M2 construct projection");
        assert_eq!(constructs.schema(), CONSTRUCT_POLICY_PROJECTION_SCHEMA);
        assert!(constructs.schema().ends_with("/1"));
        assert!(!constructs.id().as_str().is_empty());
        assert!(Arc::ptr_eq(constructs.analysis(), &analysis));
        assert_eq!(constructs.path(), path);
        assert_eq!(constructs.policy(), identity.construct_policy());
        assert_eq!(constructs.dialect().dialect(), grammar.dialect());
        assert_eq!(constructs.dialect().grammar_id(), grammar.grammar_id());
        assert_eq!(
            constructs.dialect().grammar_version(),
            grammar.grammar_version()
        );
        assert_eq!(constructs.dialect().support(), CapabilitySupport::Provided);
        assert_eq!(
            constructs.dialect().authority(),
            Some(CapabilityAuthority::Syntax)
        );
        for fact in constructs.facts() {
            assert_raw_fact(&analysis, path, fact.node(), fact.raw());
            let policy_kind = match fact.kind() {
                ConstructPolicyFactKind::UnsupportedConstruct => {
                    ConstructPolicyKind::UnsupportedConstruct
                }
                ConstructPolicyFactKind::Macro => ConstructPolicyKind::Macro,
                ConstructPolicyFactKind::GeneratedCode => ConstructPolicyKind::GeneratedCode,
                unexpected => panic!("valid M2 fixture emitted recovery fact {unexpected:?}"),
            };
            let section = constructs.policy().construct(policy_kind);
            assert_eq!(section.support(), CapabilitySupport::Provided);
            assert_eq!(section.authority(), Some(fact.authority()));
            let rule = section
                .matching_rule(fact.raw().raw_kind(), fact.text())
                .expect("M2 fact has exact declaring construct rule");
            assert_eq!(fact.construct_handling(), Some(rule.handling()));
            assert_eq!(fact.parse_handling(), None);
            construct_facts += 1;
        }

        let queries = analysis
            .compile_language_query_pack(path)
            .expect("M2 query projection");
        assert_eq!(queries.schema(), LANGUAGE_QUERY_PROJECTION_SCHEMA);
        assert!(queries.schema().ends_with("/1"));
        assert!(!queries.id().as_str().is_empty());
        assert!(Arc::ptr_eq(queries.analysis(), &analysis));
        assert_eq!(queries.path(), path);
        assert_eq!(queries.pack(), identity.queries());
        let root_node = analysis.file_node_ids(path).unwrap().next().unwrap();
        for family in QueryFamily::ALL {
            let declaration = queries.pack().declaration(family);
            let compiled = queries.query(family);
            assert_eq!(
                compiled.is_some(),
                declaration.support() == CapabilitySupport::Provided
            );
            if declaration.support() != CapabilitySupport::Provided {
                assert_eq!(declaration.authority(), None);
                assert_eq!(declaration.source(), None);
                assert!(declaration.captures().is_empty());
                continue;
            }
            assert_eq!(declaration.authority(), Some(CapabilityAuthority::Adapter));
            let compiled = compiled.unwrap();
            assert_eq!(compiled.query().grammar(), grammar);
            for matched in analysis
                .syntax_query_matches(compiled.query(), root_node)
                .expect("M2 query matches")
                .iter()
            {
                for capture in matched.captures() {
                    let capture_declaration = declaration
                        .captures()
                        .iter()
                        .find(|candidate| candidate.name() == capture.capture_name())
                        .expect("M2 capture has a declaration");
                    let node = analysis.node(capture.node()).expect("M2 capture node");
                    assert_eq!(node.path(), path);
                    let node_roles = roles_by_node
                        .get(&capture.node())
                        .expect("M2 capture has canonical-role fact");
                    for required in capture_declaration.roles().iter() {
                        assert!(
                            node_roles.contains(required),
                            "{} {} capture {} on {} lacks role {} (has {:?})",
                            row.path,
                            family.as_str(),
                            capture.capture_name(),
                            node.raw_kind(),
                            required.as_str(),
                            node_roles,
                        );
                    }
                    query_captures += 1;
                }
            }
        }
    }

    assert_eq!(analysis.node_count(), 854);
    assert_eq!(role_facts, 854);
    assert_eq!(role_assignments, 640);
    assert_eq!(lexical_facts, 536);
    assert_eq!(construct_facts, 28);
    assert_eq!(query_captures, 88);

    let mut analyzer_config = AnalyzerConfig::default();
    analyzer_config.boundary.enabled = false;
    let analyzer = scan_analysis(Arc::clone(&analysis), analyzer_config).expect("M2 analyzer");
    assert!(Arc::ptr_eq(&analyzer.analysis, &analysis));
    assert!(
        analyzer
            .reports
            .iter()
            .all(|report| report.analysis.status == AnalysisStatus::Complete
                && report.analysis.diagnostics.is_empty())
    );
    assert!(
        analyzer
            .reports
            .iter()
            .flat_map(|report| &report.findings)
            .all(|finding| finding.safety != SafetyClass::AnalyzerConfirmed),
        "syntax-only S1 adapters cannot emit analyzer-confirmed findings"
    );

    let metrics = metrics_analysis(Arc::clone(&analysis), MetricsConfig::default())
        .expect("M2 metrics projection");
    assert!(Arc::ptr_eq(&metrics.analysis, &analysis));
    assert_eq!(metrics.status, AnalysisStatus::Complete);

    let graph =
        graph_analysis(Arc::clone(&analysis), GraphConfig::default()).expect("M2 graph projection");
    assert!(Arc::ptr_eq(&graph.analysis, &analysis));
    assert_eq!(graph.status, AnalysisStatus::Complete);
    assert!(graph.edges.iter().all(|edge| {
        edge.kind == GraphEdgeKind::Contains || edge.confidence != GraphConfidence::Resolved
    }));
    assert!(
        analyzer
            .reports
            .iter()
            .filter(|report| report.path == Path::new("adapter_matrix.py"))
            .flat_map(|report| &report.findings)
            .all(|finding| finding.rule != "consecutive-blank-lines"),
        "Python's valid two-blank-line module separators are not SafeAuto findings"
    );

    assert_eq!(
        analyzer
            .reports
            .iter()
            .map(|report| report.findings.len())
            .sum::<usize>(),
        2
    );
    assert_eq!(metrics.functions.len(), 15);
    assert_eq!(graph.nodes.len(), 44);
    assert_eq!(graph.summary.symbols, 15);
    assert_eq!(graph.edges.len(), 42);
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind != GraphEdgeKind::Contains)
            .count(),
        27
    );
    assert_eq!(analysis.parse_counts(), parse_counts);

    eprintln!(
        "M2 DoD: 7 dialects/854 nodes/640 roles/536 tokens/28 constructs/88 query captures; \
         analyzer=4 findings/0 confirmed; metrics=15 regions; \
         graph=15 symbols/42 edges/27 non-containment/0 resolved non-containment; parse=7/7"
    );
}

fn assert_raw_fact(analysis: &ProjectAnalysis, path: &Path, node: NodeId, raw: &RawSyntaxFact) {
    let view = analysis.node(node).expect("M2 fact node");
    assert_eq!(view.path(), path);
    assert_eq!(raw.raw_kind(), view.raw_kind());
    assert_eq!(raw.raw_kind_id(), view.raw_kind_id());
    assert_eq!(raw.raw_grammar_kind(), view.raw_grammar_kind());
    assert_eq!(raw.raw_grammar_kind_id(), view.raw_grammar_kind_id());
    assert_eq!(raw.field(), view.field());
}
