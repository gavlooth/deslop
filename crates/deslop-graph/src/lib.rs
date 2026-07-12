use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use deslop_lang::Registry;
use deslop_parse::SourceFile;
use ignore::WalkBuilder;

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

pub fn graph_paths(paths: &[PathBuf], config: GraphConfig) -> Result<DependencyGraph> {
    let registry = Registry::default();
    let paths = deduplicate_supported_paths(collect_supported_paths(paths, &registry)?);

    let mut builder = builder::GraphBuilder::new(config);
    for path in &paths {
        builder.index_file_path(path, &registry);
    }

    for path in paths {
        let source = SourceFile::read(&path)?;
        builder.add_source(&source, &registry)?;
    }

    Ok(builder.finish())
}

fn deduplicate_supported_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    for path in paths {
        let path = normalized_display_path(&path);
        let identity = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        unique
            .entry(identity)
            .and_modify(|existing| {
                if path_precedes(&path, existing) {
                    *existing = path.to_path_buf();
                }
            })
            .or_insert(path);
    }
    unique.into_values().collect()
}

fn normalized_display_path(path: &Path) -> PathBuf {
    path.components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect()
}

fn path_precedes(candidate: &Path, current: &Path) -> bool {
    match (candidate.is_absolute(), current.is_absolute()) {
        (false, true) => true,
        (true, false) => false,
        _ => candidate < current,
    }
}

fn collect_supported_paths(paths: &[PathBuf], registry: &Registry) -> Result<Vec<PathBuf>> {
    let roots = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    let mut out = Vec::new();
    for path in roots {
        collect_supported_path(&mut out, &path, registry)?;
    }
    Ok(out)
}

fn collect_supported_path(out: &mut Vec<PathBuf>, path: &Path, registry: &Registry) -> Result<()> {
    if path.is_file() {
        if registry.supported_pack_for_path(path).is_some() {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }

    let walker = WalkBuilder::new(path)
        .hidden(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(name.as_ref(), ".git" | ".jj" | "target" | "__pycache__")
        })
        .build();

    for entry in walker {
        let entry = entry.with_context(|| format!("failed to walk {}", path.display()))?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.into_path();
        if registry.supported_pack_for_path(&path).is_some() {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::types::SCHEMA;
    use deslop_core::{Lang, Span};

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
            "graph/1 derives syntactic edges as the unclassified remainder"
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
    fn dot_render_includes_edge_labels() {
        let graph = DependencyGraph {
            schema: SCHEMA.to_string(),
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
