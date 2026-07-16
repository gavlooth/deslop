use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use deslop_parse::{
    DiscoveryPolicy, ProjectAnalysis, ProjectSnapshotPlanner, ProjectSnapshotRequest, ProjectionId,
    RepositorySpec, RootSpec, ScopeSpec, SnapshotPresentationMap,
};

mod builder;
mod extract;
mod ids;
mod render;
mod types;

pub use render::{render_dot, render_json};
pub use types::{
    DependencyGraph, GraphConfidence, GraphConfig, GraphEdge, GraphEdgeKind, GraphNode,
    GraphNodeKind, GraphNotice, GraphSummary,
};

/// Stable public dependency-graph wire schema frozen by M10.
pub const GRAPH_SCHEMA: &str = types::SCHEMA;

const GRAPH_PROJECTION_SCHEMA: &str = "deslop.graph.projection/1";
const GRAPH_CAPABILITIES: &[u8] = b"graph=deslop.graph-owned/1";

#[derive(Debug)]
pub struct GraphProjection {
    pub id: ProjectionId,
    pub analysis: Arc<ProjectAnalysis>,
    pub graph: DependencyGraph,
}

impl std::ops::Deref for GraphProjection {
    type Target = DependencyGraph;

    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}

pub fn graph_paths(paths: &[PathBuf], config: GraphConfig) -> Result<DependencyGraph> {
    let paths = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    let planner = ProjectSnapshotPlanner::resolve(ProjectSnapshotRequest {
        invocation_base: std::env::current_dir().context("resolve graph invocation base")?,
        root: RootSpec::Auto,
        repository: RepositorySpec::Auto,
        scope: ScopeSpec::Requested(paths),
        discovery: DiscoveryPolicy::LegacyRespectIgnore,
    })?;
    let built = planner.build()?;
    let analysis = ProjectAnalysis::build(built.snapshot)?;
    Ok(graph_owned_analysis(analysis, built.presentation, config)?.graph)
}

pub fn graph_analysis(
    analysis: Arc<ProjectAnalysis>,
    config: GraphConfig,
) -> Result<GraphProjection> {
    let presentation = SnapshotPresentationMap::from_entries(
        analysis
            .files()
            .map(|file| (file.key().path.clone(), file.key().path.clone())),
    )?;
    graph_owned_analysis(analysis, presentation, config)
}

fn graph_owned_analysis(
    analysis: Arc<ProjectAnalysis>,
    presentation: SnapshotPresentationMap,
    config: GraphConfig,
) -> Result<GraphProjection> {
    let presentation_entries = presentation
        .entries()
        .map(|(logical, display)| (logical.to_path_buf(), display.to_path_buf()))
        .collect::<Vec<_>>();
    let policy = serde_json::to_vec(&(config.include_calls, presentation_entries))?;
    let id = analysis.derive_projection_id(GRAPH_PROJECTION_SCHEMA, &policy, GRAPH_CAPABILITIES)?;
    let files = analysis
        .files()
        .map(|file| {
            builder::GraphFile::new(
                &analysis,
                file,
                presentation.display_path(&file.key().path).to_path_buf(),
            )
        })
        .collect::<Vec<_>>();
    let mut builder = builder::GraphBuilder::new(config);
    for file in files
        .iter()
        .filter(|file| file.file.provenance().permits_rewrites())
    {
        builder.index_file_path(&file.path, file.lang);
    }
    for file in &files {
        builder.add_source(file)?;
    }
    Ok(GraphProjection {
        id,
        analysis,
        graph: builder.finish(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::types::SCHEMA;
    use deslop_core::{Lang, Span};
    use deslop_parse::{ProjectSnapshotBuilder, RepositoryId};

    #[test]
    fn graph_production_has_static_snapshot_ownership_guards() {
        let lib = include_str!("lib.rs");
        let production = lib.split("#[cfg(test)]").next().expect("production prefix");
        for (name, source) in [
            ("graph entry", production),
            ("graph builder", include_str!("builder.rs")),
            ("graph extractor", include_str!("extract.rs")),
        ] {
            for forbidden in [
                "parse_source",
                "SourceFile::read",
                "read_to_string",
                "pack_for_path",
                "supported_pack_for_path",
                "pack_for_lang",
                "tree_sitter::Node",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "{name} reintroduced forbidden graph operation {forbidden}"
                );
            }
        }
    }

    #[test]
    fn graph_analysis_reuses_one_owned_parse_per_revision() {
        let root = tempfile::tempdir().expect("tempdir");
        deslop_parse::reset_parse_source_invocations();
        let snapshot = ProjectSnapshotBuilder::new(
            root.path(),
            RepositoryId::explicit("owned-graph-matrix").unwrap(),
        )
        .unwrap()
        .with_overlay(
            "lib.rs",
            b"mod util; fn run() { util::helper(); }\n".to_vec(),
        )
        .unwrap()
        .with_overlay("util.rs", b"pub fn helper() {}\n".to_vec())
        .unwrap()
        .build()
        .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let counts = analysis.parse_counts();

        let first = graph_analysis(analysis.clone(), GraphConfig::default()).unwrap();
        let second = graph_analysis(analysis.clone(), GraphConfig::default()).unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(render_json(&first).unwrap(), render_json(&second).unwrap());
        assert_eq!(analysis.parse_counts(), counts);
        assert_eq!(counts.len(), 2);
        assert!(counts.values().all(|count| {
            (
                count.requested,
                count.owners,
                count.parser_invocations,
                count.reused,
            ) == (1, 1, 1, 0)
        }));
        assert_eq!(deslop_parse::parse_source_invocations(), 0);
    }

    #[test]
    fn malformed_source_keeps_file_identity_without_graph_authority() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("malformed.ts");
        std::fs::write(
            &path,
            include_str!("../../../tests/fixtures/typescript/malformed.ts"),
        )
        .expect("fixture");

        let graph = graph_paths(&[path], GraphConfig::default()).expect("graph");

        assert_eq!(graph.schema, "deslop.graph/2");
        assert_eq!(graph.status, deslop_core::AnalysisStatus::Partial);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].kind, GraphNodeKind::File);
        assert!(graph.edges.is_empty());
        assert_eq!(graph.summary.symbols, 0);
        assert!(render_dot(&graph).contains("tree-sitter-error"));
    }

    #[test]
    fn rust_qualified_call_targets_the_named_module_without_claiming_resolution() {
        let temp = tempfile::tempdir().expect("tempdir");
        let lib = temp.path().join("lib.rs");
        let util = temp.path().join("util.rs");
        let other = temp.path().join("other.rs");
        std::fs::write(
            &lib,
            "mod util;\nmod other;\nuse crate::util;\n\npub fn run() {\n    util::helper();\n}\n",
        )
        .expect("write lib");
        std::fs::write(&util, "pub fn helper() {}\n").expect("write util");
        std::fs::write(&other, "pub fn helper() {}\n").expect("write other");

        let graph = graph_paths(&[temp.path().to_path_buf()], GraphConfig::default()).unwrap();
        assert_eq!(graph.schema, SCHEMA);
        assert!(
            has_node(&graph, GraphNodeKind::Function, "run"),
            "{graph:#?}"
        );
        let edge = edge_with_label(&graph, GraphEdgeKind::Calls, "util::helper");
        let target = node_named_in_file(&graph, "helper", "util.rs");
        assert_eq!(edge.confidence, GraphConfidence::Syntactic);
        assert_eq!(edge.to, target.id);
        let import = edge_with_label(&graph, GraphEdgeKind::Imports, "crate::util");
        assert_eq!(import.confidence, GraphConfidence::Syntactic);
        assert_eq!(import.to, file_node(&graph, "util.rs").id);
    }

    #[test]
    fn python_unknown_call_is_syntactic_not_proven_external() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.py");
        std::fs::write(&path, "def run():\n    print('x')\n").expect("write");

        let graph = graph_paths(&[path], GraphConfig::default()).unwrap();
        assert!(
            has_node(&graph, GraphNodeKind::ExternalSymbol, "print"),
            "{graph:#?}"
        );
        assert!(
            has_edge(
                &graph,
                GraphEdgeKind::Calls,
                GraphConfidence::Syntactic,
                "print",
            ),
            "{graph:#?}"
        );
        assert_eq!(graph.summary.external_edges, 0);
        assert_eq!(
            graph.summary.edges
                - graph.summary.resolved_edges
                - graph.summary.ambiguous_edges
                - graph.summary.external_edges,
            1,
            "graph/2 derives syntactic edges as the unclassified remainder"
        );
    }

    #[test]
    fn duplicate_names_resolve_by_file_scope_without_first_wins() {
        let temp = tempfile::tempdir().expect("tempdir");
        let left = temp.path().join("left.rs");
        let right = temp.path().join("right.rs");
        std::fs::write(&left, "fn helper() {}\nfn run() { helper(); }\n").expect("write left");
        std::fs::write(&right, "fn helper() {}\nfn run() { helper(); }\n").expect("write right");

        let graph = graph_paths(&[temp.path().to_path_buf()], GraphConfig::default()).unwrap();
        let calls = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.kind == GraphEdgeKind::Calls && edge.label.as_deref() == Some("helper")
            })
            .collect::<Vec<_>>();
        let helper_qualified_names = graph
            .nodes
            .iter()
            .filter(|node| node.name == "helper")
            .map(|node| node.qualified_name.as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(calls.len(), 2);
        assert_eq!(helper_qualified_names.len(), 2);
        for call in calls {
            assert_eq!(call.confidence, GraphConfidence::Syntactic);
            let from = graph_node(&graph, &call.from);
            let to = graph_node(&graph, &call.to);
            assert_eq!(from.path, to.path, "{call:#?}");
        }
    }

    #[test]
    fn same_scope_duplicate_definitions_are_ambiguous() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.rs");
        std::fs::write(
            &path,
            "fn helper() {}\nfn helper() {}\nfn run() { helper(); }\n",
        )
        .expect("write");

        let graph = graph_paths(&[path], GraphConfig::default()).unwrap();
        let edge = edge_with_label(&graph, GraphEdgeKind::Calls, "helper");

        assert_eq!(edge.confidence, GraphConfidence::Ambiguous);
        assert_eq!(
            graph_node(&graph, &edge.to).kind,
            GraphNodeKind::ExternalSymbol
        );
        assert!(render_dot(&graph).contains("calls: helper (ambiguous)"));
    }

    #[test]
    fn remote_duplicate_names_do_not_make_a_bare_third_file_call_look_bound() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("left.rs"), "fn helper() {}\n").expect("left");
        std::fs::write(temp.path().join("right.rs"), "fn helper() {}\n").expect("right");
        std::fs::write(temp.path().join("caller.rs"), "fn run() { helper(); }\n").expect("caller");

        let graph = graph_paths(&[temp.path().to_path_buf()], GraphConfig::default()).unwrap();
        let edge = edge_with_label(&graph, GraphEdgeKind::Calls, "helper");

        assert_eq!(edge.confidence, GraphConfidence::Syntactic);
        assert_eq!(
            graph_node(&graph, &edge.to).kind,
            GraphNodeKind::ExternalSymbol
        );
    }

    #[test]
    fn unique_remote_bare_name_is_an_unresolved_syntactic_placeholder() {
        let temp = tempfile::tempdir().expect("tempdir");
        let caller = temp.path().join("caller.rs");
        let remote = temp.path().join("remote.rs");
        std::fs::write(&caller, "fn run() { helper(); }\n").expect("write caller");
        std::fs::write(&remote, "fn helper() {}\n").expect("write remote");

        let graph = graph_paths(&[temp.path().to_path_buf()], GraphConfig::default()).unwrap();
        let edge = edge_with_label(&graph, GraphEdgeKind::Calls, "helper");

        assert_eq!(edge.confidence, GraphConfidence::Syntactic);
        assert_eq!(
            graph_node(&graph, &edge.to).kind,
            GraphNodeKind::ExternalSymbol
        );
    }

    #[test]
    fn local_binding_shadow_never_promotes_a_name_match_to_resolved() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.rs");
        std::fs::write(
            &path,
            "fn helper() {}\nfn run() { let helper = || {}; helper(); }\n",
        )
        .expect("write");

        let graph = graph_paths(&[path], GraphConfig::default()).unwrap();
        let edge = edge_with_label(&graph, GraphEdgeKind::Calls, "helper");

        assert_eq!(edge.confidence, GraphConfidence::Syntactic);
        assert_ne!(edge.confidence, GraphConfidence::Resolved);
        assert_eq!(
            graph_node(&graph, &edge.to).kind,
            GraphNodeKind::ExternalSymbol
        );
    }

    #[test]
    fn parameter_shadow_targets_an_unresolved_placeholder() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.rs");
        std::fs::write(
            &path,
            "fn helper() {}\nfn run(helper: fn()) { helper(); }\n",
        )
        .expect("write");

        let graph = graph_paths(&[path], GraphConfig::default()).unwrap();
        let edge = edge_with_label(&graph, GraphEdgeKind::Calls, "helper");

        assert_eq!(edge.confidence, GraphConfidence::Syntactic);
        assert_eq!(
            graph_node(&graph, &edge.to).kind,
            GraphNodeKind::ExternalSymbol
        );
    }

    #[test]
    fn receiver_binding_blocks_a_same_named_module_candidate() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("service.rs"), "pub fn run() {}\n").expect("service");
        std::fs::write(
            temp.path().join("caller.rs"),
            "fn invoke(service: Service) { service.run(); }\n",
        )
        .expect("caller");

        let graph = graph_paths(&[temp.path().to_path_buf()], GraphConfig::default()).unwrap();
        let edge = edge_with_label(&graph, GraphEdgeKind::Calls, "service.run");

        assert_eq!(edge.confidence, GraphConfidence::Syntactic);
        assert_eq!(
            graph_node(&graph, &edge.to).kind,
            GraphNodeKind::ExternalSymbol
        );
    }

    #[test]
    fn local_bindings_shadow_project_functions_in_each_supported_adapter() {
        let cases = [
            ShadowCase {
                extension: "py",
                source: "def helper():\n    pass\ndef run():\n    helper = lambda: None\n    helper()\n",
            },
            ShadowCase {
                extension: "js",
                source: "function helper() {}\nfunction run() { const helper = value; helper(); }\n",
            },
            ShadowCase {
                extension: "ts",
                source: "function helper() {}\nfunction run() { const helper = value; helper(); }\n",
            },
            ShadowCase {
                extension: "jl",
                source: "function helper()\nend\nfunction run()\n    helper = value\n    helper()\nend\n",
            },
            ShadowCase {
                extension: "clj",
                source: "(defn helper [] nil)\n(defn run [] (let [helper value] (helper)))\n",
            },
        ];

        for case in cases {
            let temp = tempfile::tempdir().expect("tempdir");
            let path = temp.path().join(format!("sample.{}", case.extension));
            std::fs::write(&path, case.source).expect("write shadow fixture");
            let graph = graph_paths(&[path], GraphConfig::default())
                .unwrap_or_else(|error| panic!("{} shadow graph: {error:#}", case.extension));
            let edge = edge_with_label_from_owner(&graph, GraphEdgeKind::Calls, "helper", "run");

            assert_eq!(
                edge.confidence,
                GraphConfidence::Syntactic,
                "{}",
                case.extension
            );
            assert_eq!(
                graph_node(&graph, &edge.to).kind,
                GraphNodeKind::ExternalSymbol,
                "{} local binding must block the project function",
                case.extension
            );
        }
    }

    #[test]
    fn unsupported_import_aliases_never_target_unrelated_global_names() {
        let cases = [
            AliasCase {
                extension: "rs",
                origin: "pub fn helper() {}\n",
                unrelated: "pub fn alias() {}\n",
                caller: "use crate::origin::helper as alias;\nfn run() { alias(); }\n",
                import_label: "crate::origin::helper as alias",
                call_label: "alias",
            },
            AliasCase {
                extension: "py",
                origin: "def helper():\n    pass\n",
                unrelated: "def alias():\n    pass\n",
                caller: "from origin import helper as alias\ndef run():\n    alias()\n",
                import_label: "from origin import helper as alias",
                call_label: "alias",
            },
            AliasCase {
                extension: "js",
                origin: "export function helper() {}\n",
                unrelated: "export function alias() {}\n",
                caller: "import { helper as alias } from './origin.js';\nfunction run() { alias(); }\n",
                import_label: "./origin.js",
                call_label: "alias",
            },
            AliasCase {
                extension: "ts",
                origin: "export function helper() {}\n",
                unrelated: "export function alias() {}\n",
                caller: "import { helper as alias } from './origin.ts';\nfunction run() { alias(); }\n",
                import_label: "./origin.ts",
                call_label: "alias",
            },
            AliasCase {
                extension: "jl",
                origin: "module origin\nfunction helper()\nend\nend\n",
                unrelated: "function alias()\nend\n",
                caller: "import .origin: helper as alias\nfunction run()\n    alias()\nend\n",
                import_label: ".origin: helper as alias",
                call_label: "alias",
            },
            AliasCase {
                extension: "clj",
                origin: "(ns origin)\n(defn helper [] nil)\n",
                unrelated: "(ns alias)\n(defn alias [] nil)\n",
                caller: "(ns caller (:require [origin :as o]))\n(defn run [] (o/helper))\n",
                import_label: "ns caller :require origin :as o",
                call_label: "o/helper",
            },
        ];

        for case in cases {
            assert_alias_case(case);
        }
    }

    #[test]
    fn duplicate_qualified_names_are_ambiguous_not_first_wins() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("left.rs"),
            "struct Alpha;\nimpl Alpha { fn ping() {} }\n",
        )
        .expect("left");
        std::fs::write(
            temp.path().join("right.rs"),
            "struct Alpha;\nimpl Alpha { fn ping() {} }\n",
        )
        .expect("right");
        std::fs::write(
            temp.path().join("caller.rs"),
            "fn run() { Alpha::ping(); }\n",
        )
        .expect("caller");

        let graph = graph_paths(&[temp.path().to_path_buf()], GraphConfig::default()).unwrap();
        let edge = edge_with_label(&graph, GraphEdgeKind::Calls, "Alpha::ping");

        assert_eq!(edge.confidence, GraphConfidence::Ambiguous);
    }

    #[test]
    fn colliding_module_keys_are_ambiguous_not_first_wins() {
        let temp = tempfile::tempdir().expect("tempdir");
        let left_dir = temp.path().join("left");
        let right_dir = temp.path().join("right");
        std::fs::create_dir_all(&left_dir).expect("left dir");
        std::fs::create_dir_all(&right_dir).expect("right dir");
        std::fs::write(left_dir.join("util.rs"), "fn helper() {}\n").expect("left util");
        std::fs::write(right_dir.join("util.rs"), "fn helper() {}\n").expect("right util");
        std::fs::write(
            temp.path().join("main.rs"),
            "use util;\nfn run() { util::helper(); }\n",
        )
        .expect("main");

        let graph = graph_paths(&[temp.path().to_path_buf()], GraphConfig::default()).unwrap();
        let edge = edge_with_label(&graph, GraphEdgeKind::Calls, "util::helper");
        let import = edge_with_label(&graph, GraphEdgeKind::Imports, "util");

        assert_eq!(edge.confidence, GraphConfidence::Ambiguous);
        assert_eq!(import.confidence, GraphConfidence::Ambiguous);
    }

    #[test]
    fn nested_definition_wins_the_nearest_syntactic_scope() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.rs");
        std::fs::write(
            &path,
            "fn helper() {}\nfn run() {\n    fn helper() {}\n    helper();\n}\n",
        )
        .expect("write");

        let graph = graph_paths(&[path], GraphConfig::default()).unwrap();
        let edge = edge_with_label(&graph, GraphEdgeKind::Calls, "helper");
        let target = graph_node(&graph, &edge.to);

        assert_eq!(edge.confidence, GraphConfidence::Syntactic);
        assert_eq!(target.span.expect("target span").start_line, 3);
    }

    #[test]
    fn self_and_named_type_calls_target_the_enclosing_type_not_a_peer() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.rs");
        std::fs::write(
            &path,
            "struct Alpha;\nimpl Alpha {\n    fn ping(&self) {}\n    fn run(&self) { self.ping(); Alpha::ping(self); }\n}\nstruct Beta;\nimpl Beta { fn ping(&self) {} }\n",
        )
        .expect("write");

        let graph = graph_paths(&[path], GraphConfig::default()).unwrap();
        let self_call = edge_with_label(&graph, GraphEdgeKind::Calls, "self.ping");
        let named_call = edge_with_label(&graph, GraphEdgeKind::Calls, "Alpha::ping");

        assert_eq!(self_call.confidence, GraphConfidence::Syntactic);
        assert_eq!(named_call.confidence, GraphConfidence::Syntactic);
        assert_eq!(self_call.to, named_call.to);
        assert!(
            graph_node(&graph, &self_call.to)
                .qualified_name
                .ends_with("::Alpha::ping")
        );
    }

    #[test]
    fn inheritance_edges_start_at_subclass_and_split_multiple_bases() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.py");
        std::fs::write(
            &path,
            "class Base:\n    pass\nclass Mixin:\n    pass\nclass Child(Base, Mixin):\n    pass\n",
        )
        .expect("write");

        let graph = graph_paths(&[path], GraphConfig::default()).unwrap();
        let child = graph
            .nodes
            .iter()
            .find(|node| node.name == "Child")
            .expect("Child");
        let inheritance = graph
            .edges
            .iter()
            .filter(|edge| edge.kind == GraphEdgeKind::Inherits)
            .collect::<Vec<_>>();

        assert_eq!(inheritance.len(), 2);
        assert!(inheritance.iter().all(|edge| edge.from == child.id));
        assert!(
            inheritance
                .iter()
                .all(|edge| edge.confidence == GraphConfidence::Syntactic)
        );
        assert_eq!(
            inheritance
                .iter()
                .filter_map(|edge| edge.label.as_deref())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["Base", "Mixin"])
        );
    }

    #[test]
    fn equivalent_path_order_and_spelling_produce_identical_graphs() {
        let cwd = std::env::current_dir().expect("current directory");
        let temp = tempfile::tempdir_in(&cwd).expect("tempdir in cwd");
        let absolute = temp.path().join("sample.rs");
        std::fs::write(&absolute, "fn helper() {}\nfn run() { helper(); }\n").expect("write");
        let relative = absolute.strip_prefix(&cwd).expect("relative").to_path_buf();
        let dotted = PathBuf::from(".").join(&relative);

        let forward = graph_paths(&[absolute.clone(), dotted], GraphConfig::default()).unwrap();
        let reversed = graph_paths(&[relative, absolute], GraphConfig::default()).unwrap();

        assert_eq!(
            serde_json::to_value(forward).expect("forward JSON"),
            serde_json::to_value(reversed).expect("reversed JSON")
        );
    }

    #[test]
    fn duplicate_compact_label_calls_target_the_definition_in_their_own_file() {
        let source_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let graph = graph_paths(&[source_dir], GraphConfig::default()).expect("graph crate source");
        let definitions = graph
            .nodes
            .iter()
            .filter(|node| node.name == "compact_label")
            .collect::<Vec<_>>();
        let calls = graph
            .edges
            .iter()
            .filter(|edge| {
                edge.kind == GraphEdgeKind::Calls && edge.label.as_deref() == Some("compact_label")
            })
            .collect::<Vec<_>>();

        assert_eq!(definitions.len(), 2);
        assert_eq!(calls.len(), 10);
        for call in calls {
            assert_eq!(call.confidence, GraphConfidence::Syntactic);
            assert_eq!(
                graph_node(&graph, &call.from).path,
                graph_node(&graph, &call.to).path,
                "{call:#?}"
            );
        }
    }

    #[test]
    fn graph_parses_typescript_and_tsx_with_one_language_family() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("typed.ts"),
            include_str!("../../../tests/fixtures/typescript/typed.ts"),
        )
        .expect("typescript");
        std::fs::write(
            temp.path().join("component.tsx"),
            include_str!("../../../tests/fixtures/typescript/component.tsx"),
        )
        .expect("tsx");
        std::fs::write(
            temp.path().join("component.jsx"),
            include_str!("../../../tests/fixtures/typescript/component.jsx"),
        )
        .expect("jsx");

        let graph = graph_paths(&[temp.path().to_path_buf()], GraphConfig::default()).unwrap();

        assert!(graph.notices.is_empty(), "{:#?}", graph.notices);
        assert!(graph.nodes.iter().any(|node| {
            node.name == "Entity"
                && node.kind == GraphNodeKind::Interface
                && node.lang == Lang::TypeScript
        }));
        assert!(graph.nodes.iter().any(|node| {
            node.name == "View"
                && node.kind == GraphNodeKind::Function
                && node.lang == Lang::TypeScript
        }));
        assert!(graph.nodes.iter().any(|node| {
            node.name == "JsxView"
                && node.kind == GraphNodeKind::Function
                && node.lang == Lang::JavaScript
        }));

        let json = serde_json::to_value(&graph).expect("graph JSON");
        let nodes = json["nodes"].as_array().expect("nodes");
        assert!(
            nodes
                .iter()
                .any(|node| { node["name"] == "View" && node["lang"] == "type-script" })
        );
        assert!(!nodes.iter().any(|node| node["lang"] == "tsx"));
    }

    #[test]
    fn python_graph_preserves_nested_callable_ownership_through_decorators() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("behavioral.py");
        std::fs::write(
            &path,
            include_str!("../../../tests/fixtures/python/behavioral.py"),
        )
        .expect("Python fixture");
        let graph = graph_paths(&[path], GraphConfig::default()).expect("Python graph");

        assert!(graph.notices.is_empty(), "{:#?}", graph.notices);
        let file = file_node(&graph, "behavioral.py");
        let traced = node_named_in_file(&graph, "traced", "behavioral.py");
        let wrapper = node_named_in_file(&graph, "wrapper", "behavioral.py");
        let service = node_named_in_file(&graph, "Service", "behavioral.py");
        let process = node_named_in_file(&graph, "process", "behavioral.py");
        let normalize = node_named_in_file(&graph, "normalize", "behavioral.py");

        assert_eq!(traced.kind, GraphNodeKind::Function);
        assert_eq!(wrapper.kind, GraphNodeKind::Function);
        assert_eq!(service.kind, GraphNodeKind::Class);
        assert_eq!(process.kind, GraphNodeKind::Method);
        assert_eq!(normalize.kind, GraphNodeKind::Function);
        for (owner, child) in [
            (&file.id, &traced.id),
            (&traced.id, &wrapper.id),
            (&file.id, &service.id),
            (&service.id, &process.id),
            (&process.id, &normalize.id),
        ] {
            assert!(graph.edges.iter().any(|edge| {
                edge.kind == GraphEdgeKind::Contains
                    && edge.from == *owner
                    && edge.to == *child
                    && edge.confidence == GraphConfidence::Resolved
            }));
        }
        assert_eq!(
            graph
                .nodes
                .iter()
                .filter(|node| node.name == "decorated_definition")
                .count(),
            0
        );
    }

    #[test]
    fn dot_render_includes_edge_labels() {
        let graph = DependencyGraph {
            schema: SCHEMA.to_string(),
            status: deslop_core::AnalysisStatus::Complete,
            analyses: Vec::new(),
            summary: GraphSummary {
                files: 0,
                symbols: 0,
                external_symbols: 0,
                edges: 1,
                resolved_edges: 1,
                ambiguous_edges: 0,
                external_edges: 0,
            },
            agent_notes: Vec::new(),
            nodes: vec![
                test_node("a", GraphNodeKind::File),
                test_node("b", GraphNodeKind::Function),
            ],
            edges: vec![GraphEdge {
                kind: GraphEdgeKind::Contains,
                from: "a".to_string(),
                to: "b".to_string(),
                confidence: GraphConfidence::Resolved,
                label: Some("b".to_string()),
                span: Some(Span::new(1, 1, 0, 0)),
            }],
            notices: Vec::new(),
        };
        assert!(render_dot(&graph).contains("contains: b"));
    }

    fn test_node(id: &str, kind: GraphNodeKind) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            kind,
            lang: Lang::Rust,
            path: None,
            name: id.to_string(),
            qualified_name: id.to_string(),
            span: None,
            signature: None,
        }
    }

    #[derive(Clone, Copy)]
    struct AliasCase {
        extension: &'static str,
        origin: &'static str,
        unrelated: &'static str,
        caller: &'static str,
        import_label: &'static str,
        call_label: &'static str,
    }

    #[derive(Clone, Copy)]
    struct ShadowCase {
        extension: &'static str,
        source: &'static str,
    }

    fn assert_alias_case(case: AliasCase) {
        let temp = tempfile::tempdir().expect("tempdir");
        let origin = temp.path().join(format!("origin.{}", case.extension));
        let unrelated = temp.path().join(format!("alias.{}", case.extension));
        let caller = temp.path().join(format!("caller.{}", case.extension));
        std::fs::write(&origin, case.origin).expect("write origin");
        std::fs::write(&unrelated, case.unrelated).expect("write unrelated");
        std::fs::write(&caller, case.caller).expect("write caller");

        let graph = graph_paths(&[temp.path().to_path_buf()], GraphConfig::default())
            .unwrap_or_else(|error| panic!("{} alias graph: {error:#}", case.extension));
        let call = edge_with_label_in_file(
            &graph,
            GraphEdgeKind::Calls,
            case.call_label,
            &format!("caller.{}", case.extension),
        );
        let import = edge_with_label_in_file(
            &graph,
            GraphEdgeKind::Imports,
            case.import_label,
            &format!("caller.{}", case.extension),
        );

        assert_eq!(
            call.confidence,
            GraphConfidence::Syntactic,
            "{}",
            case.extension
        );
        assert_eq!(
            graph_node(&graph, &call.to).kind,
            GraphNodeKind::ExternalSymbol,
            "{} must not target alias.{}",
            case.extension,
            case.extension
        );
        assert_eq!(
            import.confidence,
            GraphConfidence::Syntactic,
            "{}",
            case.extension
        );
        assert_eq!(
            import.to,
            file_node(&graph, &format!("origin.{}", case.extension)).id
        );
        if case.extension == "clj" {
            assert!(!graph.edges.iter().any(|edge| {
                edge.kind == GraphEdgeKind::Calls && edge.label.as_deref() == Some(":require")
            }));
        }
    }

    fn has_node(graph: &DependencyGraph, kind: GraphNodeKind, name: &str) -> bool {
        graph
            .nodes
            .iter()
            .any(|node| node.kind == kind && node.name == name)
    }

    fn graph_node<'a>(graph: &'a DependencyGraph, id: &str) -> &'a GraphNode {
        graph
            .nodes
            .iter()
            .find(|node| node.id == id)
            .unwrap_or_else(|| panic!("missing graph node {id}"))
    }

    fn node_named_in_file<'a>(
        graph: &'a DependencyGraph,
        name: &str,
        file_name: &str,
    ) -> &'a GraphNode {
        graph
            .nodes
            .iter()
            .find(|node| {
                node.name == name
                    && node
                        .path
                        .as_deref()
                        .is_some_and(|path| path.ends_with(file_name))
            })
            .unwrap_or_else(|| panic!("missing {name} in {file_name}"))
    }

    fn file_node<'a>(graph: &'a DependencyGraph, file_name: &str) -> &'a GraphNode {
        graph
            .nodes
            .iter()
            .find(|node| {
                node.kind == GraphNodeKind::File
                    && node
                        .path
                        .as_deref()
                        .is_some_and(|path| path.ends_with(file_name))
            })
            .unwrap_or_else(|| panic!("missing file node {file_name}"))
    }

    fn edge_with_label<'a>(
        graph: &'a DependencyGraph,
        kind: GraphEdgeKind,
        label: &str,
    ) -> &'a GraphEdge {
        graph
            .edges
            .iter()
            .find(|edge| edge.kind == kind && edge.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("missing {kind:?} edge {label}"))
    }

    fn edge_with_label_in_file<'a>(
        graph: &'a DependencyGraph,
        kind: GraphEdgeKind,
        label: &str,
        file_name: &str,
    ) -> &'a GraphEdge {
        graph
            .edges
            .iter()
            .find(|edge| {
                edge.kind == kind
                    && edge.label.as_deref() == Some(label)
                    && graph_node(graph, &edge.from)
                        .path
                        .as_deref()
                        .is_some_and(|path| path.ends_with(file_name))
            })
            .unwrap_or_else(|| panic!("missing {kind:?} edge {label} in {file_name}"))
    }

    fn edge_with_label_from_owner<'a>(
        graph: &'a DependencyGraph,
        kind: GraphEdgeKind,
        label: &str,
        owner_name: &str,
    ) -> &'a GraphEdge {
        graph
            .edges
            .iter()
            .find(|edge| {
                edge.kind == kind
                    && edge.label.as_deref() == Some(label)
                    && graph_node(graph, &edge.from).name == owner_name
            })
            .unwrap_or_else(|| panic!("missing {kind:?} edge {label} from {owner_name}"))
    }

    fn has_edge(
        graph: &DependencyGraph,
        kind: GraphEdgeKind,
        confidence: GraphConfidence,
        label: &str,
    ) -> bool {
        graph.edges.iter().any(|edge| {
            edge.kind == kind
                && edge.confidence == confidence
                && edge.label.as_deref() == Some(label)
        })
    }
}
