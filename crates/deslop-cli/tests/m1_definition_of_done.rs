use std::fs;
use std::path::Path;
use std::sync::Arc;

use deslop_analyzer::{AnalyzerConfig, scan_analysis, scan_paths_with_context};
use deslop_graph::{GraphConfig, graph_analysis, render_json as render_graph_json};
use deslop_metrics::{MetricsConfig, metrics_analysis, render_json as render_metrics_json};
use deslop_protocol::propose_work_orders;

#[test]
fn m1_owned_analysis_definition_of_done_is_numerically_locked() {
    let root = tempfile::tempdir().expect("gold root");
    let sources = [
        (
            "gold/rust.rs",
            include_bytes!("../../../tests/corpus/sloppy/rust_idioms.rs").as_slice(),
        ),
        (
            "gold/python.py",
            include_bytes!("../../../tests/corpus/sloppy/python_idioms.py").as_slice(),
        ),
        (
            "gold/component.tsx",
            include_bytes!("../../../tests/fixtures/typescript/component.tsx").as_slice(),
        ),
        (
            "gold/clojure.clj",
            include_bytes!("../../../tests/corpus/sloppy/clojure_idioms.clj").as_slice(),
        ),
        (
            "gold/julia.jl",
            include_bytes!("../../../tests/corpus/sloppy/julia_idioms.jl").as_slice(),
        ),
    ];
    let mut paths = Vec::new();
    for (relative, source) in sources {
        let path = root.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).expect("gold parent");
        fs::write(&path, source).expect("gold source");
        paths.push(path);
    }

    deslop_parse::reset_parse_source_invocations();
    let path_scan = scan_paths_with_context(&paths, AnalyzerConfig::default()).expect("path scan");
    let analysis = Arc::clone(&path_scan.analysis);
    let cold = analysis.instrumentation();
    assert!(cold.parse.invariant_holds());
    assert_eq!(cold.parse.file_revisions, 5);
    assert_eq!(
        (
            cold.parse.requested,
            cold.parse.owners,
            cold.parse.parser_invocations,
            cold.parse.reused,
        ),
        (5, 5, 5, 0)
    );
    assert_eq!(analysis.snapshot().read_counts().len(), 5);
    assert!(
        analysis
            .snapshot()
            .read_counts()
            .values()
            .all(|reads| *reads == 1)
    );

    let mut analyzer_config = AnalyzerConfig::default();
    analyzer_config.boundary.enabled = false;
    let analyzer = scan_analysis(Arc::clone(&analysis), analyzer_config.clone()).expect("analyzer");
    let analyzer_again =
        scan_analysis(Arc::clone(&analysis), analyzer_config).expect("repeated analyzer");
    let metrics =
        metrics_analysis(Arc::clone(&analysis), MetricsConfig::default()).expect("metrics");
    let metrics_again =
        metrics_analysis(Arc::clone(&analysis), MetricsConfig::default()).expect("repeat metrics");
    let graph = graph_analysis(Arc::clone(&analysis), GraphConfig::default()).expect("graph");
    let graph_again =
        graph_analysis(Arc::clone(&analysis), GraphConfig::default()).expect("repeat graph");

    for projected in [&analyzer.analysis, &metrics.analysis, &graph.analysis] {
        assert!(Arc::ptr_eq(projected, &analysis));
        assert_eq!(projected.id(), analysis.id());
    }
    assert_eq!(analyzer.id, analyzer_again.id);
    assert_eq!(metrics.id, metrics_again.id);
    assert_eq!(graph.id, graph_again.id);
    assert_eq!(
        serde_json::to_value(&analyzer.reports).unwrap(),
        serde_json::to_value(&analyzer_again.reports).unwrap()
    );
    assert_eq!(
        render_metrics_json(&metrics).unwrap(),
        render_metrics_json(&metrics_again).unwrap()
    );
    assert_eq!(
        render_graph_json(&graph).unwrap(),
        render_graph_json(&graph_again).unwrap()
    );
    assert_eq!(analysis.parse_counts(), analyzer.analysis.parse_counts());

    let mut exclusive_regions = 0;
    for file in analysis.files() {
        let mut next = 0;
        for region in analysis
            .exclusive_syntax_regions(&file.key().path)
            .expect("owned syntax")
        {
            let range = region.byte_range();
            assert_eq!(
                range.start,
                next,
                "gap or overlap in {}",
                file.key().path.display()
            );
            assert!(range.start < range.end);
            next = range.end;
            exclusive_regions += 1;
        }
        assert_eq!(next, file.source().len());
    }

    let warm = analysis
        .successor(Arc::clone(analysis.snapshot()))
        .expect("warm successor");
    let warm_parse = warm.current().instrumentation().parse;
    assert!(warm_parse.invariant_holds());
    assert_eq!(
        (
            warm_parse.requested,
            warm_parse.owners,
            warm_parse.parser_invocations,
            warm_parse.reused,
        ),
        (5, 5, 0, 5)
    );
    assert_eq!(warm.current().id(), analysis.id());
    assert_eq!(
        warm.instrumentation().retained_transitions,
        analysis.node_count()
    );

    let proposal =
        propose_work_orders(root.path(), &paths, AnalyzerConfig::default()).expect("gold proposal");
    let proposal_parse = proposal.analysis.instrumentation().parse;
    assert!(proposal_parse.invariant_holds());
    assert_eq!(proposal_parse.file_revisions, 5);
    assert_eq!(
        (
            proposal_parse.requested,
            proposal_parse.owners,
            proposal_parse.parser_invocations,
            proposal_parse.reused,
        ),
        (5, 5, 5, 0)
    );
    assert_eq!(proposal.reports.len(), 5);
    assert!(
        proposal
            .work_orders
            .iter()
            .all(|order| order.path.starts_with(Path::new("gold")))
    );
    assert_eq!(deslop_parse::parse_source_invocations(), 0);

    assert_eq!(analysis.node_count(), 746);
    assert_eq!(
        analysis
            .files()
            .map(|file| file.source().len())
            .sum::<usize>(),
        1_651
    );
    assert_eq!(exclusive_regions, 700);
    assert_eq!(
        analyzer
            .reports
            .iter()
            .map(|report| report.findings.len())
            .sum::<usize>(),
        21
    );
    assert_eq!(metrics.functions.len(), 17);
    assert_eq!(graph.nodes.len(), 45);
    assert_eq!(graph.edges.len(), 49);
    assert_eq!(proposal.work_orders.len(), 9);
    assert_eq!(
        proposal
            .work_orders
            .iter()
            .map(|order| order.findings.len())
            .sum::<usize>(),
        17
    );
    assert_eq!(warm.instrumentation().retained_transitions, 746);
}
