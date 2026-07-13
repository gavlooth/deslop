use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use anyhow::{Context, Result};
use deslop_core::{AnalysisStatus, FileAnalysis, Lang, Span, file_analyses_status};
use deslop_lang::{LangPack, RegionClass, RegionSpan, Registry};
use deslop_parse::{
    NodeId, ParsedFile, ProjectAnalysis, SourceFile, SyntaxAdapterFacts,
    analysis_provenance_or_failed, parse_source,
};
use ignore::WalkBuilder;
use serde::Serialize;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy)]
pub struct MetricsConfig {
    pub sigma: f64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self { sigma: 2.0 }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsReport {
    pub schema: &'static str,
    pub status: AnalysisStatus,
    pub analyses: Vec<FileAnalysis>,
    pub functions: Vec<RegionMetrics>,
    pub heuristic_outliers: Vec<HeuristicBurdenOutlier>,
    pub heuristic_burden_distribution: Option<BurdenDistribution>,
    pub hotspots: Vec<Hotspot>,
    pub heuristic_model: HeuristicBurdenModel,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionMetrics {
    pub path: PathBuf,
    pub lang: Lang,
    pub name: String,
    pub kind: String,
    pub span: Span,
    pub complexity: ComplexityMetrics,
    pub expressivity: ExpressivityMetrics,
    pub halstead: HalsteadMetrics,
    pub heuristic_burden: HeuristicBurdenMetrics,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct HeuristicBurdenModel {
    pub id: &'static str,
    pub experimental: bool,
    pub human_calibrated: bool,
    pub authority: &'static str,
    pub gating_permitted: bool,
    pub meaning: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ComplexityMetrics {
    pub cyclomatic: f64,
    pub cognitive: f64,
    pub max_nesting: usize,
    pub nloc: usize,
    pub maintainability_index: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ExpressivityMetrics {
    pub tokens: usize,
    pub vocabulary: usize,
    pub decision_density: f64,
    pub unique_token_ratio: f64,
    pub comment_to_code_ratio: f64,
    pub byte_entropy_bits_per_byte: f64,
    pub token_entropy: f64,
    pub structural_entropy: f64,
    pub information_volume: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct HeuristicBurdenMetrics {
    pub score: f64,
    pub measurement_support: f64,
    pub basis: &'static str,
    pub repo_relative: Option<RepoRelativeBurden>,
    pub size_support: f64,
    pub complexity_burden: f64,
    pub information_burden: f64,
    pub entropy_burden: f64,
    pub interaction_burden: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct HalsteadMetrics {
    pub distinct_operators: usize,
    pub distinct_operands: usize,
    pub total_operators: usize,
    pub total_operands: usize,
    pub volume: f64,
    pub difficulty: f64,
    pub lexical_effort: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hotspot {
    pub rank: usize,
    pub path: PathBuf,
    pub name: String,
    pub span: Span,
    pub score: f64,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeuristicBurdenOutlier {
    pub rank: usize,
    pub path: PathBuf,
    pub name: String,
    pub kind: String,
    pub span: Span,
    pub heuristic_burden: f64,
    pub measurement_support: f64,
    pub basis: &'static str,
    pub repo_relative: RepoRelativeBurden,
    pub size_support: f64,
    pub reasons: Vec<String>,
}

const RELATIVE_BURDEN_Z_THRESHOLD: f64 = 1.0;
const RELATIVE_BURDEN_PERCENTILE_THRESHOLD: f64 = 0.90;
const MIN_RELATIVE_REGIONS: usize = 8;
const MIN_BURDEN_RANGE: f64 = 0.05;
const MIN_BURDEN_STDDEV: f64 = 0.01;
const HEURISTIC_BASIS: &str = "tree_heuristic_v1";

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BurdenDistribution {
    pub count: usize,
    pub mean: f64,
    pub median: f64,
    pub stddev: f64,
    pub min: f64,
    pub max: f64,
    pub p25: f64,
    pub p75: f64,
    pub flat: bool,
    pub relative_outlier_eligible: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RepoRelativeBurden {
    pub zscore: f64,
    pub percentile: f64,
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    is_comment: bool,
}

pub fn metrics_paths(paths: &[PathBuf], config: MetricsConfig) -> Result<MetricsReport> {
    let mut functions = Vec::new();
    let mut analyses = Vec::new();
    for path in input_files(paths)? {
        let source = SourceFile::read(&path)?;
        let analysis = analysis_provenance_or_failed(&source);
        analyses.push(FileAnalysis {
            path: source.path.clone(),
            lang: source.lang,
            analysis: analysis.clone(),
        });
        if analysis.permits_rewrites() {
            functions.extend(metrics_source(&source)?);
        }
    }
    functions.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.span.start_line.cmp(&b.span.start_line))
            .then(a.name.cmp(&b.name))
    });
    analyses.sort_by(|a, b| a.path.cmp(&b.path));
    let status = file_analyses_status(&analyses);
    let authoritative = status == AnalysisStatus::Complete;
    let heuristic_burden_distribution =
        authoritative.then(|| normalize_heuristic_burden(&mut functions));
    let heuristic_outliers = if let Some(distribution) = heuristic_burden_distribution {
        detect_heuristic_outliers(&functions, distribution)
    } else {
        Vec::new()
    };
    let hotspots = if authoritative {
        detect_hotspots(&functions, config.sigma)
    } else {
        Vec::new()
    };
    Ok(MetricsReport {
        schema: "deslop.metrics/5",
        status,
        analyses,
        functions,
        heuristic_outliers,
        heuristic_burden_distribution,
        hotspots,
        heuristic_model: HeuristicBurdenModel {
            id: "deslop-heuristic-burden/1",
            experimental: true,
            human_calibrated: false,
            authority: "triage_only",
            gating_permitted: false,
            meaning: "hand-set structural burden evidence for triage only; not readability, health, refactor need, probability, confidence, or safety",
        },
    })
}

/// Compute metrics from one already-owned immutable project analysis without reading or parsing.
pub fn metrics_analysis(
    analysis: &ProjectAnalysis,
    config: MetricsConfig,
) -> Result<MetricsReport> {
    let mut functions = Vec::new();
    let mut analyses = Vec::new();
    for file in analysis.files() {
        analyses.push(FileAnalysis {
            path: file.key().path.clone(),
            lang: file.grammar().lang(),
            analysis: file.provenance().clone(),
        });
        if file.provenance().permits_rewrites() {
            functions.extend(metrics_file(analysis, file)?);
        }
    }
    finish_metrics_report(analyses, functions, config)
}

fn finish_metrics_report(
    mut analyses: Vec<FileAnalysis>,
    mut functions: Vec<RegionMetrics>,
    config: MetricsConfig,
) -> Result<MetricsReport> {
    functions.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.span.start_line.cmp(&b.span.start_line))
            .then(a.name.cmp(&b.name))
    });
    analyses.sort_by(|a, b| a.path.cmp(&b.path));
    let status = file_analyses_status(&analyses);
    let authoritative = status == AnalysisStatus::Complete;
    let heuristic_burden_distribution =
        authoritative.then(|| normalize_heuristic_burden(&mut functions));
    let heuristic_outliers = heuristic_burden_distribution.map_or_else(Vec::new, |distribution| {
        detect_heuristic_outliers(&functions, distribution)
    });
    let hotspots = authoritative
        .then(|| detect_hotspots(&functions, config.sigma))
        .unwrap_or_default();
    Ok(MetricsReport {
        schema: "deslop.metrics/5",
        status,
        analyses,
        functions,
        heuristic_outliers,
        heuristic_burden_distribution,
        hotspots,
        heuristic_model: HeuristicBurdenModel {
            id: "deslop-heuristic-burden/1",
            experimental: true,
            human_calibrated: false,
            authority: "triage_only",
            gating_permitted: false,
            meaning: "hand-set structural burden evidence for triage only; not readability, health, refactor need, probability, confidence, or safety",
        },
    })
}

struct MetricFile<'analysis> {
    analysis: &'analysis ProjectAnalysis,
    file: &'analysis ParsedFile,
    facts: Box<[SyntaxAdapterFacts]>,
    facts_by_node: HashMap<NodeId, usize>,
}

impl<'analysis> MetricFile<'analysis> {
    fn new(analysis: &'analysis ProjectAnalysis, file: &'analysis ParsedFile) -> Result<Self> {
        let facts = analysis.syntax_adapter_facts(&file.key().path)?;
        let facts_by_node = facts
            .iter()
            .enumerate()
            .map(|(index, facts)| (facts.node(), index))
            .collect();
        Ok(Self {
            analysis,
            file,
            facts,
            facts_by_node,
        })
    }

    fn text(&self) -> &str {
        self.file
            .text()
            .expect("complete metrics file has UTF-8 text")
    }

    fn fact(&self, node: NodeId) -> &SyntaxAdapterFacts {
        &self.facts[self.facts_by_node[&node]]
    }
}

fn metrics_file(analysis: &ProjectAnalysis, file: &ParsedFile) -> Result<Vec<RegionMetrics>> {
    let context = MetricFile::new(analysis, file)?;
    let pack = Registry::default().pack_for_lang(file.grammar().lang());
    let regions = metric_regions_owned(pack, &context)?;
    Ok(regions
        .into_iter()
        .map(|region| measure_region_owned(pack, &context, region))
        .collect())
}

pub fn metrics_source(source: &SourceFile) -> Result<Vec<RegionMetrics>> {
    if !analysis_provenance_or_failed(source).permits_rewrites() {
        return Ok(Vec::new());
    }
    let registry = Registry::default();
    let pack = registry.pack_for_lang(source.lang);
    let regions = metric_regions(pack, source)?;
    Ok(regions
        .into_iter()
        .map(|region| measure_region(pack, source, region))
        .collect())
}

pub fn render_text(report: &MetricsReport, hotspots_only: bool) -> String {
    let mut out = String::new();
    for file in &report.analyses {
        for diagnostic in &file.analysis.diagnostics {
            let location = diagnostic.span.map_or_else(
                || file.path.display().to_string(),
                |span| format!("{}:{}", file.path.display(), span.start_line),
            );
            out.push_str(&format!(
                "{location} [{}] {}\n",
                diagnostic.code, diagnostic.message
            ));
        }
    }
    out.push_str(&metrics_summary_line(report));
    if !hotspots_only {
        out.push_str(&regions_text(&report.functions));
    }
    out.push_str(&heuristic_outliers_text(&report.heuristic_outliers));
    out.push_str(&hotspots_text(&report.hotspots));
    out
}

fn metrics_summary_line(report: &MetricsReport) -> String {
    let Some(distribution) = report.heuristic_burden_distribution else {
        return format!(
            "Experimental heuristic burden: per-region evidence only ({}; requested snapshot incomplete)\nBurden distribution: unavailable\n",
            report.heuristic_model.id,
        );
    };
    format!(
        "Experimental heuristic burden: {} region(s), {} scan-local outlier(s) ({}; not health/readability/refactor authority)\nBurden distribution: n={} mean={:.3} std={:.3} median={:.3} p25={:.3} p75={:.3} min={:.3} max={:.3} flat={} outlier-eligible={}\n",
        report.functions.len(),
        report.heuristic_outliers.len(),
        report.heuristic_model.id,
        distribution.count,
        distribution.mean,
        distribution.stddev,
        distribution.median,
        distribution.p25,
        distribution.p75,
        distribution.min,
        distribution.max,
        distribution.flat,
        distribution.relative_outlier_eligible,
    )
}

fn regions_text(functions: &[RegionMetrics]) -> String {
    let mut out = String::from(
        "\nregion                                kind          burden support     z   pct cyc cog nest nloc   MI  dens uniq byteH  tokH  astH   info\n",
    );
    for region in functions {
        out.push_str(&region_text_line(region));
    }
    out
}

fn region_text_line(region: &RegionMetrics) -> String {
    let (zscore, percentile) = region.heuristic_burden.repo_relative.map_or_else(
        || ("n/a".to_string(), "n/a".to_string()),
        |relative| {
            (
                format!("{:.2}", relative.zscore),
                format!("{:.3}", relative.percentile),
            )
        },
    );
    format!(
        "{:<37} {:<13} {:>6.3} {:>6.3} {:>5} {:>5} {:>3.0} {:>3.0} {:>4} {:>4} {:>5.1} {:>5.3} {:>4.2} {:>5.3} {:>5.3} {:>5.3} {:>6.1}\n",
        short_name(region),
        region.kind,
        region.heuristic_burden.score,
        region.heuristic_burden.measurement_support,
        zscore,
        percentile,
        region.complexity.cyclomatic,
        region.complexity.cognitive,
        region.complexity.max_nesting,
        region.complexity.nloc,
        region.complexity.maintainability_index,
        region.expressivity.decision_density,
        region.expressivity.unique_token_ratio,
        region.expressivity.byte_entropy_bits_per_byte,
        region.expressivity.token_entropy,
        region.expressivity.structural_entropy,
        region.expressivity.information_volume,
    )
}

fn hotspots_text(hotspots: &[Hotspot]) -> String {
    let mut out = String::from("\nhotspots\n");
    if hotspots.is_empty() {
        out.push_str("  none\n");
    } else {
        for hotspot in hotspots {
            out.push_str(&hotspot_text_line(hotspot));
        }
    }
    out
}

fn heuristic_outliers_text(outliers: &[HeuristicBurdenOutlier]) -> String {
    let mut out = String::from("\nscan-local heuristic burden outliers\n");
    if outliers.is_empty() {
        out.push_str("  none\n");
        return out;
    }
    for outlier in outliers {
        out.push_str(&format!(
            "  #{:<2} {:<39} kind={} burden={:.3} support={:.3} z={:.2} percentile={:.3} {}\n",
            outlier.rank,
            format!(
                "{}:{} {}",
                outlier.path.display(),
                outlier.span.start_line,
                outlier.name
            ),
            outlier.kind,
            outlier.heuristic_burden,
            outlier.measurement_support,
            outlier.repo_relative.zscore,
            outlier.repo_relative.percentile,
            outlier.reasons.join(", "),
        ));
    }
    out
}

fn hotspot_text_line(hotspot: &Hotspot) -> String {
    format!(
        "  #{:<2} {:<43} score={:.2} {}\n",
        hotspot.rank,
        format!("{}:{}", hotspot.path.display(), hotspot.span.start_line),
        hotspot.score,
        hotspot.reasons.join(", ")
    )
}

pub fn render_json(report: &MetricsReport) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}

pub fn halstead_for_text(pack: &dyn LangPack, text: &str) -> HalsteadMetrics {
    halstead(&tokenize(text, pack.line_comments()), pack)
}

fn input_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let paths = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    };
    let registry = Registry::default();
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            if registry.supported_pack_for_path(&path).is_some() {
                files.push(path);
            }
            continue;
        }
        let walker = WalkBuilder::new(&path)
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
                files.push(path);
            }
        }
    }
    Ok(files)
}

fn metric_regions(pack: &dyn LangPack, source: &SourceFile) -> Result<Vec<MetricRegion>> {
    let Some(tree) = parse_source(source)? else {
        return Ok(vec![whole_file_region(source, None)]);
    };
    if tree.root_node().has_error() {
        return Ok(vec![whole_file_region(source, None)]);
    }
    let root_range = Some(node_range(tree.root_node()));
    if pack.metrics_regions().is_empty() {
        return Ok(vec![whole_file_region(source, root_range)]);
    }
    let mut regions = Vec::new();
    collect_regions(tree.root_node(), pack, &source.text, &mut regions);
    if regions.is_empty() {
        regions.push(whole_file_region(source, root_range));
    }
    Ok(regions)
}

fn metric_regions_owned(pack: &dyn LangPack, source: &MetricFile<'_>) -> Result<Vec<MetricRegion>> {
    let root = source
        .analysis
        .file_node_ids(&source.file.key().path)
        .and_then(|mut ids| ids.next())
        .expect("complete syntax file owns a root node");
    let root_view = source
        .analysis
        .node(root)
        .expect("root id is analysis-owned");
    if root_view.has_error() {
        return Ok(vec![whole_file_region_owned(source, None)]);
    }
    if pack.metrics_regions().is_empty() {
        return Ok(vec![whole_file_region_owned(source, Some(root))]);
    }
    let mut regions = Vec::new();
    collect_regions_owned(root, pack, source, &mut regions);
    if regions.is_empty() {
        regions.push(whole_file_region_owned(source, Some(root)));
    }
    Ok(regions)
}

fn collect_regions_owned(
    node: NodeId,
    pack: &dyn LangPack,
    source: &MetricFile<'_>,
    regions: &mut Vec<MetricRegion>,
) {
    let view = source.analysis.node(node).expect("node is analysis-owned");
    let kind = view.raw_kind();
    let declared_region = pack.metrics_regions().contains(&kind);
    let semantic_region =
        kind != "list_lit" || source.fact(node).region_class() != RegionClass::Other;
    if declared_region && semantic_region {
        regions.push(MetricRegion {
            name: region_name_owned(node, source),
            kind: kind.to_string(),
            span: source
                .fact(node)
                .enclosing_region()
                .unwrap_or_else(|| region_from_view(view.span(), source.text().len())),
            node: Some(node),
            legacy_node: None,
        });
    }
    for child in view.children() {
        collect_regions_owned(child, pack, source, regions);
    }
}

fn whole_file_region_owned(source: &MetricFile<'_>, node: Option<NodeId>) -> MetricRegion {
    MetricRegion {
        name: "file".to_string(),
        kind: "file".to_string(),
        span: RegionSpan {
            start_line: 1,
            end_line: source.text().lines().count().max(1),
            start_byte: 0,
            end_byte: source.text().len(),
        },
        node,
        legacy_node: None,
    }
}

fn collect_regions(
    node: Node<'_>,
    pack: &dyn LangPack,
    text: &str,
    regions: &mut Vec<MetricRegion>,
) {
    let declared_region = pack.metrics_regions().contains(&node.kind());
    let semantic_region =
        node.kind() != "list_lit" || pack.region_class(node, text) != RegionClass::Other;
    if declared_region && semantic_region {
        regions.push(MetricRegion {
            name: region_name(node, text),
            kind: node.kind().to_string(),
            span: pack
                .enclosing_region(node, text)
                .unwrap_or_else(|| region_from_node(node, text)),
            node: None,
            legacy_node: Some(node_range(node)),
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_regions(child, pack, text, regions);
    }
}

fn whole_file_region(source: &SourceFile, node: Option<NodeRange>) -> MetricRegion {
    let end_line = source.lines().len().max(1);
    MetricRegion {
        name: "file".to_string(),
        kind: "file".to_string(),
        span: RegionSpan {
            start_line: 1,
            end_line,
            start_byte: 0,
            end_byte: source.text.len(),
        },
        node: None,
        legacy_node: node,
    }
}

fn node_range(node: Node<'_>) -> NodeRange {
    NodeRange {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
    }
}

fn measure_region(pack: &dyn LangPack, source: &SourceFile, region: MetricRegion) -> RegionMetrics {
    let text = source
        .text
        .get(region.span.start_byte..region.span.end_byte)
        .unwrap_or("");
    let tokens = tokenize(text, pack.line_comments());
    let halstead = halstead(&tokens, pack);
    let ast = ast_stats_for_region(pack, source, region.legacy_node);
    let nloc = nloc(text, pack.line_comments());
    let cyclomatic = ast.branch_count as f64 + 1.0;
    let maintainability_index = maintainability_index(halstead.volume, cyclomatic, nloc);
    let complexity = complexity_metrics(ast, cyclomatic, nloc, maintainability_index);
    let expressivity = expressivity(
        text,
        &tokens,
        cyclomatic,
        nloc,
        pack.line_comments(),
        ast.information,
    );
    let heuristic_burden = heuristic_burden_metrics(
        &complexity,
        &expressivity,
        ast.node_count,
        region.legacy_node.is_some(),
    );
    RegionMetrics {
        path: source.path.clone(),
        lang: source.lang,
        name: region.name,
        kind: region.kind,
        span: span_from_region(region.span),
        complexity,
        expressivity,
        halstead,
        heuristic_burden,
    }
}

fn measure_region_owned(
    pack: &dyn LangPack,
    source: &MetricFile<'_>,
    region: MetricRegion,
) -> RegionMetrics {
    let text = source
        .text()
        .get(region.span.start_byte..region.span.end_byte)
        .unwrap_or("");
    let tokens = tokenize(text, pack.line_comments());
    let halstead = halstead(&tokens, pack);
    let ast = region
        .node
        .map(|node| ast_complexity_owned(node, source))
        .unwrap_or_default();
    let nloc = nloc(text, pack.line_comments());
    let cyclomatic = ast.branch_count as f64 + 1.0;
    let maintainability_index = maintainability_index(halstead.volume, cyclomatic, nloc);
    let complexity = complexity_metrics(ast, cyclomatic, nloc, maintainability_index);
    let expressivity = expressivity(
        text,
        &tokens,
        cyclomatic,
        nloc,
        pack.line_comments(),
        ast.information,
    );
    let heuristic_burden = heuristic_burden_metrics(
        &complexity,
        &expressivity,
        ast.node_count,
        region.node.is_some(),
    );
    RegionMetrics {
        path: source.file.key().path.clone(),
        lang: source.file.grammar().lang(),
        name: region.name,
        kind: region.kind,
        span: span_from_region(region.span),
        complexity,
        expressivity,
        halstead,
        heuristic_burden,
    }
}

fn ast_stats_for_region(
    pack: &dyn LangPack,
    source: &SourceFile,
    node: Option<NodeRange>,
) -> AstStats {
    node.and_then(|range| {
        parse_source(source).ok().flatten().and_then(|tree| {
            tree.root_node()
                .descendant_for_byte_range(range.start_byte, range.end_byte)
                .map(|node| ast_complexity(node, pack, &source.text))
        })
    })
    .unwrap_or_default()
}

fn span_from_region(span: RegionSpan) -> Span {
    Span::new(
        span.start_line,
        span.end_line,
        span.start_byte,
        span.end_byte,
    )
}

fn complexity_metrics(
    ast: AstStats,
    cyclomatic: f64,
    nloc: usize,
    maintainability_index: f64,
) -> ComplexityMetrics {
    ComplexityMetrics {
        cyclomatic,
        cognitive: ast.cognitive as f64,
        max_nesting: ast.max_nesting,
        nloc,
        maintainability_index,
    }
}

fn ast_complexity(node: Node<'_>, pack: &dyn LangPack, text: &str) -> AstStats {
    fn visit(
        node: Node<'_>,
        pack: &dyn LangPack,
        text: &str,
        nesting: usize,
        stats: &mut AstStats,
        kinds: &mut BTreeMap<String, usize>,
        leaf_tokens: &mut BTreeMap<String, usize>,
    ) {
        let kind = node.kind();
        stats.node_count += 1;
        *kinds.entry(kind.to_string()).or_insert(0) += 1;
        if node.child_count() == 0 && !kind.contains("comment") {
            let token = node.utf8_text(text.as_bytes()).unwrap_or(kind);
            if !token.trim().is_empty() {
                *leaf_tokens.entry(token.to_string()).or_insert(0) += 1;
            }
        }
        let branch_contribution = pack.metric_branch_contribution(node, text);
        let is_nesting = pack.is_metric_nesting(node, text);
        if branch_contribution > 0 {
            stats.branch_count += branch_contribution;
            stats.cognitive += branch_contribution * (1 + nesting);
        }
        if pack.is_metric_flow_break(node, text) {
            stats.cognitive += 1;
        }
        let next_nesting = if is_nesting { nesting + 1 } else { nesting };
        stats.max_nesting = stats.max_nesting.max(next_nesting);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            visit(child, pack, text, next_nesting, stats, kinds, leaf_tokens);
        }
    }
    let mut stats = AstStats::default();
    let mut kinds = BTreeMap::new();
    let mut leaf_tokens = BTreeMap::new();
    visit(
        node,
        pack,
        text,
        0,
        &mut stats,
        &mut kinds,
        &mut leaf_tokens,
    );
    stats.information =
        information_stats(&leaf_tokens, normalized_entropy(kinds.values().copied()));
    stats
}

fn ast_complexity_owned(node: NodeId, source: &MetricFile<'_>) -> AstStats {
    fn visit(
        node: NodeId,
        source: &MetricFile<'_>,
        nesting: usize,
        stats: &mut AstStats,
        kinds: &mut BTreeMap<String, usize>,
        leaf_tokens: &mut BTreeMap<String, usize>,
    ) {
        let view = source.analysis.node(node).expect("node is analysis-owned");
        let kind = view.raw_kind();
        stats.node_count += 1;
        *kinds.entry(kind.to_string()).or_insert(0) += 1;
        if view.is_leaf() && !kind.contains("comment") {
            let token = view.text();
            if !token.trim().is_empty() {
                *leaf_tokens.entry(token.to_string()).or_insert(0) += 1;
            }
        }
        let fact = source.fact(node);
        let branch_contribution = fact.metric_branch_contribution();
        if branch_contribution > 0 {
            stats.branch_count += branch_contribution;
            stats.cognitive += branch_contribution * (1 + nesting);
        }
        if fact.is_metric_flow_break() {
            stats.cognitive += 1;
        }
        let next_nesting = nesting + usize::from(fact.is_metric_nesting());
        stats.max_nesting = stats.max_nesting.max(next_nesting);
        for child in view.children() {
            visit(child, source, next_nesting, stats, kinds, leaf_tokens);
        }
    }
    let mut stats = AstStats::default();
    let mut kinds = BTreeMap::new();
    let mut leaf_tokens = BTreeMap::new();
    visit(node, source, 0, &mut stats, &mut kinds, &mut leaf_tokens);
    stats.information =
        information_stats(&leaf_tokens, normalized_entropy(kinds.values().copied()));
    stats
}

#[derive(Debug, Clone)]
struct MetricRegion {
    name: String,
    kind: String,
    span: RegionSpan,
    node: Option<NodeId>,
    legacy_node: Option<NodeRange>,
}

#[derive(Debug, Clone, Copy)]
struct NodeRange {
    start_byte: usize,
    end_byte: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct AstStats {
    branch_count: usize,
    cognitive: usize,
    max_nesting: usize,
    node_count: usize,
    information: InformationStats,
}

#[derive(Debug, Clone, Copy, Default)]
struct InformationStats {
    tokens: usize,
    vocabulary: usize,
    token_entropy: f64,
    structural_entropy: f64,
    information_volume: f64,
}

fn tokenize(text: &str, comment_tokens: &[&str]) -> Vec<Token> {
    let mut tokens = Vec::new();
    for line in text.lines() {
        let comment_at = comment_tokens
            .iter()
            .filter_map(|token| line.find(token))
            .min();
        let (code, comment) = match comment_at {
            Some(idx) => (&line[..idx], Some(&line[idx..])),
            None => (line, None),
        };
        tokens.extend(tokenize_code(code, false));
        if let Some(comment) = comment {
            tokens.extend(tokenize_code(comment, true));
        }
    }
    tokens
}

fn tokenize_code(text: &str, is_comment: bool) -> Vec<Token> {
    let mut out = Vec::new();
    let mut iter = text.char_indices().peekable();
    while let Some((start, ch)) = iter.next() {
        if ch.is_whitespace() {
            continue;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' {
            let end = consume_word(&mut iter, start, ch);
            out.push(token_from_slice(text, start, end, is_comment));
            continue;
        }
        if let Some(token) = consume_two_char_operator(text, &mut iter, start, is_comment) {
            out.push(token);
            continue;
        }
        out.push(Token {
            text: ch.to_string(),
            is_comment,
        });
    }
    out
}

fn consume_word(
    iter: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
    first: char,
) -> usize {
    let mut end = start + first.len_utf8();
    while let Some((idx, next)) = iter.peek().copied() {
        if next.is_ascii_alphanumeric() || next == '_' {
            iter.next();
            end = idx + next.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn consume_two_char_operator(
    text: &str,
    iter: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
    is_comment: bool,
) -> Option<Token> {
    let (idx, next) = iter.peek().copied()?;
    let end = idx + next.len_utf8();
    let two = &text[start..end];
    if is_two_char_operator(two) {
        iter.next();
        Some(token_from_slice(text, start, end, is_comment))
    } else {
        None
    }
}

fn is_two_char_operator(value: &str) -> bool {
    matches!(
        value,
        "==" | "!=" | "<=" | ">=" | "&&" | "||" | "<<" | ">>" | "+=" | "-=" | "*=" | "/=" | "%="
    )
}

fn token_from_slice(text: &str, start: usize, end: usize, is_comment: bool) -> Token {
    Token {
        text: text[start..end].to_string(),
        is_comment,
    }
}

fn halstead(tokens: &[Token], pack: &dyn LangPack) -> HalsteadMetrics {
    let operators = pack.halstead_operator_tokens();
    let mut distinct_operators = BTreeSet::new();
    let mut distinct_operands = BTreeSet::new();
    let mut total_operators = 0;
    let mut total_operands = 0;
    for token in tokens.iter().filter(|token| !token.is_comment) {
        if operators.contains(&token.text.as_str()) {
            distinct_operators.insert(token.text.clone());
            total_operators += 1;
        } else {
            distinct_operands.insert(token.text.clone());
            total_operands += 1;
        }
    }
    let n1 = distinct_operators.len();
    let n2 = distinct_operands.len();
    let big_n = total_operators + total_operands;
    let vocabulary = n1 + n2;
    let volume = if vocabulary == 0 {
        0.0
    } else {
        big_n as f64 * (vocabulary as f64).log2()
    };
    let difficulty = if n2 == 0 {
        0.0
    } else {
        (n1 as f64 / 2.0) * (total_operands as f64 / n2 as f64)
    };
    HalsteadMetrics {
        distinct_operators: n1,
        distinct_operands: n2,
        total_operators,
        total_operands,
        volume,
        difficulty,
        lexical_effort: volume * difficulty,
    }
}

fn expressivity(
    text: &str,
    tokens: &[Token],
    cyclomatic: f64,
    nloc: usize,
    comment_tokens: &[&str],
    tree_sitter_information: InformationStats,
) -> ExpressivityMetrics {
    let code_tokens: Vec<_> = tokens.iter().filter(|token| !token.is_comment).collect();
    let token_counts = code_tokens
        .iter()
        .fold(BTreeMap::new(), |mut counts, token| {
            *counts.entry(token.text.as_str()).or_insert(0usize) += 1;
            counts
        });
    let fallback_information = information_stats(&token_counts, 0.0);
    let information = if tree_sitter_information.tokens > 0 {
        tree_sitter_information
    } else {
        fallback_information
    };
    let comment_lines = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            comment_tokens
                .iter()
                .any(|token| trimmed.starts_with(token))
        })
        .count();
    ExpressivityMetrics {
        tokens: information.tokens,
        vocabulary: information.vocabulary,
        decision_density: ratio(cyclomatic, information.tokens),
        unique_token_ratio: ratio(information.vocabulary as f64, information.tokens),
        comment_to_code_ratio: ratio(comment_lines as f64, nloc),
        byte_entropy_bits_per_byte: byte_entropy_bits_per_byte(text),
        token_entropy: information.token_entropy,
        structural_entropy: information.structural_entropy,
        information_volume: information.information_volume,
    }
}

fn information_stats<K: Ord>(
    counts: &BTreeMap<K, usize>,
    structural_entropy: f64,
) -> InformationStats {
    let tokens = counts.values().sum::<usize>();
    let token_entropy_bits = shannon_entropy(counts.values().copied());
    InformationStats {
        tokens,
        vocabulary: counts.len(),
        token_entropy: normalized_entropy(counts.values().copied()),
        structural_entropy,
        information_volume: token_entropy_bits * tokens as f64,
    }
}

fn heuristic_burden_metrics(
    complexity: &ComplexityMetrics,
    expressivity: &ExpressivityMetrics,
    ast_nodes: usize,
    ast_available: bool,
) -> HeuristicBurdenMetrics {
    let complexity_burden = 0.50 * saturating(complexity.cognitive, 10.0)
        + 0.30 * saturating((complexity.cyclomatic - 1.0).max(0.0), 8.0)
        + 0.20 * saturating(complexity.max_nesting as f64, 4.0);
    let information_burden = saturating(expressivity.information_volume, 512.0);
    let token_support = saturating(expressivity.tokens as f64, 16.0);
    let structural_support = if ast_available {
        saturating(ast_nodes as f64, 32.0)
    } else {
        0.0
    };
    let lexical_redundancy = (1.0 - expressivity.token_entropy) * token_support;
    let structural_disorder =
        expressivity.structural_entropy * structural_support * complexity_burden;
    let entropy_burden = 0.60 * lexical_redundancy + 0.40 * structural_disorder;
    let interaction_burden =
        complexity_burden * (0.65 * information_burden + 0.35 * expressivity.structural_entropy);
    let total_burden = (0.45 * complexity_burden
        + 0.20 * information_burden
        + 0.15 * entropy_burden
        + 0.20 * interaction_burden)
        .clamp(0.0, 1.0);
    let measurement_support =
        (0.20 + 0.45 * saturating(expressivity.tokens as f64, 32.0) + 0.30 * structural_support)
            .clamp(0.0, 0.95);
    let size_support =
        saturating(expressivity.tokens as f64, 64.0).max(saturating(complexity.nloc as f64, 20.0));
    let heuristic_burden =
        (total_burden * (0.50 + size_support)).clamp(0.0, 1.0) * measurement_support;
    HeuristicBurdenMetrics {
        score: heuristic_burden,
        measurement_support,
        basis: HEURISTIC_BASIS,
        repo_relative: None,
        size_support,
        complexity_burden,
        information_burden,
        entropy_burden,
        interaction_burden,
    }
}

fn saturating(value: f64, half_saturation: f64) -> f64 {
    if value <= 0.0 {
        0.0
    } else {
        value / (value + half_saturation)
    }
}

fn nloc(text: &str, comment_tokens: &[&str]) -> usize {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && !comment_tokens
                    .iter()
                    .any(|token| trimmed.starts_with(token))
        })
        .count()
}

fn maintainability_index(volume: f64, cyclomatic: f64, nloc: usize) -> f64 {
    if nloc == 0 {
        return 100.0;
    }
    let volume = volume.max(1.0);
    let raw = 171.0 - 5.2 * volume.ln() - 0.23 * cyclomatic - 16.2 * (nloc as f64).ln();
    (raw * 100.0 / 171.0).clamp(0.0, 100.0)
}

fn normalize_heuristic_burden(functions: &mut [RegionMetrics]) -> BurdenDistribution {
    let values = functions
        .iter()
        .map(|region| region.heuristic_burden.score)
        .collect::<Vec<_>>();
    let (distribution, normalized) = burden_normalization(&values);
    for (region, (zscore, percentile)) in functions.iter_mut().zip(normalized) {
        region.heuristic_burden.repo_relative = Some(RepoRelativeBurden { zscore, percentile });
    }
    distribution
}

fn burden_normalization(values: &[f64]) -> (BurdenDistribution, Vec<(f64, f64)>) {
    if values.is_empty() {
        return (
            BurdenDistribution {
                count: 0,
                mean: 0.0,
                median: 0.0,
                stddev: 0.0,
                min: 0.0,
                max: 0.0,
                p25: 0.0,
                p75: 0.0,
                flat: true,
                relative_outlier_eligible: false,
            },
            Vec::new(),
        );
    }
    let values = values
        .iter()
        .map(|value| {
            if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            }
        })
        .collect::<Vec<_>>();
    let mut sorted = values.clone();
    sorted.sort_by(f64::total_cmp);
    let count = sorted.len();
    let mean = sorted.iter().sum::<f64>() / count as f64;
    let stddev = (sorted
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / count as f64)
        .sqrt();
    let min = sorted[0];
    let max = sorted[count - 1];
    let flat = count < 2 || max - min < MIN_BURDEN_RANGE || stddev < MIN_BURDEN_STDDEV;
    let distribution = BurdenDistribution {
        count,
        mean,
        median: quantile(&sorted, 0.50),
        stddev,
        min,
        max,
        p25: quantile(&sorted, 0.25),
        p75: quantile(&sorted, 0.75),
        flat,
        relative_outlier_eligible: count >= MIN_RELATIVE_REGIONS && !flat,
    };
    let mut indexed = values.iter().copied().enumerate().collect::<Vec<_>>();
    indexed.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut percentiles = vec![0.5; count];
    let mut start = 0;
    while start < count {
        let mut end = start + 1;
        while end < count && indexed[end].1.total_cmp(&indexed[start].1).is_eq() {
            end += 1;
        }
        let percentile = if count == 1 {
            0.5
        } else {
            ((start + end - 1) as f64 / 2.0) / (count - 1) as f64
        };
        for &(original_index, _) in &indexed[start..end] {
            percentiles[original_index] = percentile;
        }
        start = end;
    }
    let normalized = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let zscore = if !flat && stddev > 0.0 {
                (value - mean) / stddev
            } else {
                0.0
            };
            (zscore, percentiles[index])
        })
        .collect();
    (distribution, normalized)
}

fn quantile(sorted: &[f64], probability: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let position = probability.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let fraction = position - lower as f64;
        sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
    }
}

fn detect_heuristic_outliers(
    functions: &[RegionMetrics],
    distribution: BurdenDistribution,
) -> Vec<HeuristicBurdenOutlier> {
    let mut outliers = functions
        .iter()
        .filter(|region| is_heuristic_outlier(region.heuristic_burden, distribution))
        .map(heuristic_outlier)
        .collect::<Vec<_>>();
    outliers.sort_by(|a, b| {
        b.repo_relative
            .percentile
            .total_cmp(&a.repo_relative.percentile)
            .then(b.repo_relative.zscore.total_cmp(&a.repo_relative.zscore))
            .then(b.heuristic_burden.total_cmp(&a.heuristic_burden))
            .then(a.path.cmp(&b.path))
            .then(a.span.start_line.cmp(&b.span.start_line))
    });
    for (index, outlier) in outliers.iter_mut().enumerate() {
        outlier.rank = index + 1;
    }
    outliers
}

fn is_heuristic_outlier(burden: HeuristicBurdenMetrics, distribution: BurdenDistribution) -> bool {
    let Some(relative) = burden.repo_relative else {
        return false;
    };
    distribution.relative_outlier_eligible
        && relative.zscore >= RELATIVE_BURDEN_Z_THRESHOLD
        && relative.percentile >= RELATIVE_BURDEN_PERCENTILE_THRESHOLD
}

fn heuristic_outlier(region: &RegionMetrics) -> HeuristicBurdenOutlier {
    let burden = region.heuristic_burden;
    let mut reasons = Vec::new();
    push_burden_reason(&mut reasons, "complexity", burden.complexity_burden);
    push_burden_reason(&mut reasons, "information", burden.information_burden);
    push_burden_reason(&mut reasons, "entropy", burden.entropy_burden);
    push_burden_reason(
        &mut reasons,
        "complexity×information",
        burden.interaction_burden,
    );
    reasons.push(format!("size-support={:.2}", burden.size_support));
    let relative = burden
        .repo_relative
        .expect("heuristic outliers require complete repo-relative evidence");
    reasons.push(format!(
        "scan-relative-z={:.2}, percentile={:.3}",
        relative.zscore, relative.percentile
    ));
    HeuristicBurdenOutlier {
        rank: 0,
        path: region.path.clone(),
        name: region.name.clone(),
        kind: region.kind.clone(),
        span: region.span,
        heuristic_burden: burden.score,
        measurement_support: burden.measurement_support,
        basis: burden.basis,
        repo_relative: relative,
        size_support: burden.size_support,
        reasons,
    }
}

fn push_burden_reason(reasons: &mut Vec<String>, name: &str, burden: f64) {
    if burden >= 0.20 {
        reasons.push(format!("{name}={burden:.2}"));
    }
}

fn detect_hotspots(functions: &[RegionMetrics], sigma: f64) -> Vec<Hotspot> {
    let distributions = MetricDistributions::new(functions);
    let mut hotspots = functions
        .iter()
        .filter_map(|region| hotspot_for_region(region, &distributions, sigma))
        .collect::<Vec<_>>();
    rank_hotspots(&mut hotspots);
    hotspots
}

fn hotspot_for_region(
    region: &RegionMetrics,
    distributions: &MetricDistributions,
    sigma: f64,
) -> Option<Hotspot> {
    let mut score = 0.0;
    let mut reasons = Vec::new();
    check_complexity_hotspots(region, distributions, sigma, &mut score, &mut reasons);
    check_expressivity_hotspots(region, distributions, sigma, &mut score, &mut reasons);
    if reasons.is_empty() {
        return None;
    }
    Some(Hotspot {
        rank: 0,
        path: region.path.clone(),
        name: region.name.clone(),
        span: region.span,
        score,
        reasons,
    })
}

fn check_complexity_hotspots(
    region: &RegionMetrics,
    distributions: &MetricDistributions,
    sigma: f64,
    score: &mut f64,
    reasons: &mut Vec<String>,
) {
    let checks = [
        (
            "cyclomatic",
            region.complexity.cyclomatic,
            distributions.cyclomatic,
        ),
        (
            "cognitive",
            region.complexity.cognitive,
            distributions.cognitive,
        ),
        ("nloc", region.complexity.nloc as f64, distributions.nloc),
        (
            "halstead-lexical-effort",
            region.halstead.lexical_effort,
            distributions.lexical_effort,
        ),
    ];
    for (name, value, distribution) in checks {
        check_high(name, value, distribution, sigma, score, reasons);
    }
}

fn check_expressivity_hotspots(
    region: &RegionMetrics,
    distributions: &MetricDistributions,
    sigma: f64,
    score: &mut f64,
    reasons: &mut Vec<String>,
) {
    if region.expressivity.tokens >= 16 {
        let checks = [
            (
                "decision-density",
                region.expressivity.decision_density,
                distributions.decision_density,
            ),
            (
                "unique-token-ratio",
                region.expressivity.unique_token_ratio,
                distributions.unique_token_ratio,
            ),
        ];
        for (name, value, distribution) in checks {
            check_low(name, value, distribution, sigma, score, reasons);
        }
    }
    check_high(
        "comment-ratio",
        region.expressivity.comment_to_code_ratio,
        distributions.comment_to_code_ratio,
        sigma,
        score,
        reasons,
    );
}

fn rank_hotspots(hotspots: &mut [Hotspot]) {
    hotspots.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.path.cmp(&b.path)));
    for (idx, hotspot) in hotspots.iter_mut().enumerate() {
        hotspot.rank = idx + 1;
    }
}

fn check_high(
    name: &str,
    value: f64,
    distribution: Distribution,
    sigma: f64,
    score: &mut f64,
    reasons: &mut Vec<String>,
) {
    let threshold = distribution.median + sigma * distribution.stddev;
    if distribution.stddev > 0.0 && value >= threshold {
        let z = (value - distribution.median) / distribution.stddev;
        *score += z;
        reasons.push(format!("{name} high z={z:.2}"));
    }
}

fn check_low(
    name: &str,
    value: f64,
    distribution: Distribution,
    sigma: f64,
    score: &mut f64,
    reasons: &mut Vec<String>,
) {
    let threshold = distribution.median - sigma * distribution.stddev;
    if distribution.stddev > 0.0 && value <= threshold {
        let z = (distribution.median - value) / distribution.stddev;
        *score += z;
        reasons.push(format!("{name} low z={z:.2}"));
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Distribution {
    median: f64,
    stddev: f64,
}

struct MetricDistributions {
    cyclomatic: Distribution,
    cognitive: Distribution,
    nloc: Distribution,
    lexical_effort: Distribution,
    decision_density: Distribution,
    unique_token_ratio: Distribution,
    comment_to_code_ratio: Distribution,
}

impl MetricDistributions {
    fn new(functions: &[RegionMetrics]) -> Self {
        Self {
            cyclomatic: distribution(functions.iter().map(|region| region.complexity.cyclomatic)),
            cognitive: distribution(functions.iter().map(|region| region.complexity.cognitive)),
            nloc: distribution(functions.iter().map(|region| region.complexity.nloc as f64)),
            lexical_effort: distribution(
                functions
                    .iter()
                    .map(|region| region.halstead.lexical_effort),
            ),
            decision_density: distribution(
                functions
                    .iter()
                    .map(|region| region.expressivity.decision_density),
            ),
            unique_token_ratio: distribution(
                functions
                    .iter()
                    .map(|region| region.expressivity.unique_token_ratio),
            ),
            comment_to_code_ratio: distribution(
                functions
                    .iter()
                    .map(|region| region.expressivity.comment_to_code_ratio),
            ),
        }
    }
}

fn distribution(values: impl Iterator<Item = f64>) -> Distribution {
    let mut values = values.filter(|value| value.is_finite()).collect::<Vec<_>>();
    if values.is_empty() {
        return Distribution::default();
    }
    values.sort_by(f64::total_cmp);
    let median = if values.len() % 2 == 0 {
        (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
    } else {
        values[values.len() / 2]
    };
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    Distribution {
        median,
        stddev: variance.sqrt(),
    }
}

fn shannon_entropy(counts: impl Iterator<Item = usize>) -> f64 {
    let counts = counts.filter(|count| *count > 0).collect::<Vec<_>>();
    let total = counts.iter().sum::<usize>() as f64;
    if total == 0.0 {
        return 0.0;
    }
    counts
        .into_iter()
        .map(|count| {
            let probability = count as f64 / total;
            -probability * probability.log2()
        })
        .sum()
}

fn normalized_entropy(counts: impl Iterator<Item = usize>) -> f64 {
    let counts = counts.filter(|count| *count > 0).collect::<Vec<_>>();
    if counts.len() <= 1 {
        return 0.0;
    }
    (shannon_entropy(counts.iter().copied()) / (counts.len() as f64).log2()).clamp(0.0, 1.0)
}

fn byte_entropy_bits_per_byte(text: &str) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    let mut counts = BTreeMap::new();
    for byte in text.bytes() {
        *counts.entry(byte).or_insert(0usize) += 1;
    }
    let len = text.len() as f64;
    counts
        .values()
        .map(|count| {
            let probability = *count as f64 / len;
            -probability * probability.log2()
        })
        .sum::<f64>()
}

fn ratio(numerator: f64, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator / denominator as f64
    }
}

fn region_from_node(node: Node<'_>, text: &str) -> RegionSpan {
    let start_position = node.start_position();
    let end_position = node.end_position();
    let mut end_line = end_position.row + 1;
    if end_position.column == 0 && end_line > start_position.row + 1 {
        end_line -= 1;
    }
    RegionSpan {
        start_line: start_position.row + 1,
        end_line,
        start_byte: node.start_byte(),
        end_byte: node.end_byte().min(text.len()),
    }
}

fn region_from_view(span: deslop_parse::SyntaxSpan, text_len: usize) -> RegionSpan {
    let start = span.start_point();
    let end = span.end_point();
    let mut end_line = end.row() + 1;
    if end.column() == 0 && end_line > start.row() + 1 {
        end_line -= 1;
    }
    RegionSpan {
        start_line: start.row() + 1,
        end_line,
        start_byte: span.start_byte(),
        end_byte: span.end_byte().min(text_len),
    }
}

fn region_name(node: Node<'_>, text: &str) -> String {
    if let Some(name) = node.child_by_field_name("name") {
        return name
            .utf8_text(text.as_bytes())
            .unwrap_or(node.kind())
            .to_string();
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind().contains("identifier") {
            return child
                .utf8_text(text.as_bytes())
                .unwrap_or(node.kind())
                .to_string();
        }
    }
    node.kind().to_string()
}

fn region_name_owned(node: NodeId, source: &MetricFile<'_>) -> String {
    let view = source.analysis.node(node).expect("node is analysis-owned");
    let children = view.children();
    if let Some(name) = children.iter().find_map(|child| {
        let child = source
            .analysis
            .node(*child)
            .expect("child is analysis-owned");
        (child.field() == Some("name")).then_some(child)
    }) {
        return name.text().to_string();
    }
    children
        .into_iter()
        .find_map(|child| {
            let child = source
                .analysis
                .node(child)
                .expect("child is analysis-owned");
            child
                .raw_kind()
                .contains("identifier")
                .then(|| child.text().to_string())
        })
        .unwrap_or_else(|| view.raw_kind().to_string())
}

fn short_name(region: &RegionMetrics) -> String {
    format!(
        "{}:{} {}",
        region.path.display(),
        region.span.start_line,
        region.name
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use deslop_lang::RUST_PACK;
    use deslop_parse::{ProjectAnalysis, ProjectSnapshotBuilder, RepositoryId};

    #[test]
    fn cyclomatic_counts_known_rust_branches() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn f(x: i32) -> i32 {\n  if x > 0 { 1 } else { match x { 0 => 0, _ => -1 } }\n}\n"
                .to_string(),
        );
        let report = metrics_source(&source).expect("metrics");
        let function = report.iter().find(|region| region.name == "f").expect("f");
        assert_eq!(function.complexity.cyclomatic, 4.0);
    }

    #[test]
    fn malformed_input_has_no_structural_metrics_or_aggregate_scores() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let malformed = tmp.path().join("malformed.ts");
        std::fs::write(
            &malformed,
            include_str!("../../../tests/fixtures/typescript/malformed.ts"),
        )
        .expect("fixture");

        let report = metrics_paths(&[malformed], MetricsConfig::default()).expect("metrics");

        assert_eq!(report.schema, "deslop.metrics/5");
        assert_eq!(report.status, AnalysisStatus::Partial);
        assert!(report.functions.is_empty());
        assert!(report.heuristic_outliers.is_empty());
        assert!(report.hotspots.is_empty());
        assert!(report.heuristic_burden_distribution.is_none());
    }

    #[test]
    fn mixed_partial_scan_withholds_project_level_metric_authority() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let valid = tmp.path().join("valid.rs");
        let malformed = tmp.path().join("malformed.ts");
        std::fs::write(&valid, "fn valid() -> i32 { 1 }\n").expect("valid fixture");
        std::fs::write(
            &malformed,
            include_str!("../../../tests/fixtures/typescript/malformed.ts"),
        )
        .expect("malformed fixture");

        let report = metrics_paths(&[valid, malformed], MetricsConfig::default()).expect("metrics");

        assert_eq!(report.status, AnalysisStatus::Partial);
        assert!(!report.functions.is_empty());
        assert!(report.heuristic_outliers.is_empty());
        assert!(report.hotspots.is_empty());
        assert!(report.heuristic_burden_distribution.is_none());
        assert!(
            report
                .functions
                .iter()
                .all(|region| region.heuristic_burden.repo_relative.is_none())
        );
        let text = render_text(&report, false);
        assert!(text.contains("Burden distribution: unavailable"));
        assert!(text.contains("  n/a   n/a"));
    }

    #[test]
    fn halstead_known_numbers() {
        let halstead = halstead_for_text(&RUST_PACK, "a + b * c");
        assert_eq!(halstead.distinct_operators, 2);
        assert_eq!(halstead.total_operators, 2);
        assert_eq!(halstead.distinct_operands, 3);
        assert_eq!(halstead.total_operands, 3);
        assert!((halstead.volume - 11.609_640).abs() < 0.000_01);
        assert!((halstead.difficulty - 1.0).abs() < 0.000_01);
        assert!((halstead.lexical_effort - 11.609_640).abs() < 0.000_01);
    }

    #[test]
    fn byte_entropy_uses_bits_per_byte_and_not_a_compression_label() {
        assert_close(byte_entropy_bits_per_byte("aaaa"), 0.0);
        assert_close(byte_entropy_bits_per_byte("ab"), 1.0);

        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn sample(value: i32) -> i32 { value + 1 }\n".to_string(),
        );
        let region = metrics_source(&source)
            .expect("metrics")
            .into_iter()
            .find(|region| region.name == "sample")
            .expect("sample region");
        let json = serde_json::to_value(region.expressivity).expect("expressivity JSON");
        assert!(json["byte_entropy_bits_per_byte"].is_number());
        assert!(json.get("compression_ratio").is_none());
    }

    #[test]
    fn hotspot_detection_flags_bloated_outlier_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("sample.rs");
        let branches = (0..40)
            .map(|value| format!("  if x == {value} {{ return {value}; }}\n"))
            .collect::<String>();
        std::fs::write(
            &path,
            format!(
                "fn clean_a() -> i32 {{ 1 }}\nfn clean_b() -> i32 {{ 2 }}\nfn clean_c() -> i32 {{ 3 }}\nfn bloated(x: i32) -> i32 {{\n{branches}  x\n}}\n"
            ),
        )
        .expect("fixture");
        let report = metrics_paths(&[path], MetricsConfig { sigma: 1.0 }).expect("metrics");
        let bloated = report
            .functions
            .iter()
            .find(|region| region.name == "bloated")
            .expect("bloated region");
        assert!(bloated.heuristic_burden.score > 0.50);
        assert!(
            report
                .hotspots
                .iter()
                .any(|hotspot| hotspot.name == "bloated")
        );
        assert!(
            report
                .hotspots
                .iter()
                .all(|hotspot| !hotspot.name.starts_with("clean_"))
        );
        assert!(report.heuristic_outliers.is_empty());
        assert!(
            !report
                .heuristic_burden_distribution
                .expect("complete distribution")
                .relative_outlier_eligible
        );
    }

    #[test]
    fn text_output_uses_neutral_experimental_labels() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("sample.rs");
        std::fs::write(&path, "fn sample(value: i32) -> i32 { value + 1 }\n").expect("fixture");
        let report = metrics_paths(&[path], MetricsConfig::default()).expect("metrics");
        let text = render_text(&report, false);
        assert!(text.contains("Experimental heuristic burden"));
        assert!(text.contains("scan-local heuristic burden outliers"));
        for forbidden in [
            "Repo health:",
            "Structural readability:",
            "Refactor confidence distribution:",
            "readability refactor candidates",
            "confidence=",
        ] {
            assert!(!text.contains(forbidden), "unexpected text {forbidden}");
        }
        let json = serde_json::to_value(&report).expect("metrics JSON");
        assert_eq!(json["schema"], "deslop.metrics/5");
        assert_eq!(json["heuristic_model"]["authority"], "triage_only");
        assert_eq!(json["heuristic_model"]["gating_permitted"], false);
        for forbidden in [
            "health_score",
            "readability_score",
            "readability_model",
            "refactor_candidates",
            "refactor_confidence_distribution",
        ] {
            assert!(json.get(forbidden).is_none(), "unexpected key {forbidden}");
        }
    }

    #[test]
    fn tree_sitter_supplies_entropy_and_heuristic_burden_evidence() {
        let source = SourceFile::new(
            PathBuf::from("sample.rs"),
            "fn readable(value: i32) -> i32 { value + 1 }\n".to_string(),
        );
        let report = metrics_source(&source).expect("metrics");
        let function = report
            .iter()
            .find(|region| region.name == "readable")
            .expect("readable");
        assert!(function.expressivity.tokens > 0);
        assert!(function.expressivity.vocabulary > 0);
        assert!(function.expressivity.token_entropy > 0.0);
        assert!(function.expressivity.structural_entropy > 0.0);
        assert!(function.expressivity.information_volume > 0.0);
        assert!((0.0..=1.0).contains(&function.heuristic_burden.score));
        assert!(function.heuristic_burden.measurement_support > 0.20);
        assert_eq!(function.heuristic_burden.basis, HEURISTIC_BASIS);
    }

    #[test]
    fn metrics_analysis_uses_one_owned_parse_and_never_touches_the_legacy_counter() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let path = root.join("tests/fixtures/python/behavioral.py");
        deslop_parse::reset_parse_source_invocations();
        let snapshot = ProjectSnapshotBuilder::new(&root, RepositoryId::local(&root).unwrap())
            .unwrap()
            .with_exact_files(&[path])
            .build()
            .unwrap();
        let analysis = ProjectAnalysis::build(snapshot).unwrap();
        let counts_before = analysis.parse_counts();
        let report = metrics_analysis(&analysis, MetricsConfig::default()).expect("metrics");
        let repeated = metrics_analysis(&analysis, MetricsConfig::default()).expect("metrics");

        assert_eq!(report.functions.len(), 5);
        assert_eq!(
            render_json(&report).unwrap(),
            render_json(&repeated).unwrap()
        );
        assert_eq!(analysis.parse_counts(), counts_before);
        assert_eq!(counts_before.len(), 1);
        assert_eq!(counts_before.values().next().unwrap().parser_invocations, 1);
        assert_eq!(deslop_parse::parse_source_invocations(), 0);
    }

    #[test]
    fn trivial_helpers_do_not_change_intrinsic_target_metrics() {
        let target = "fn target(x: i32) -> i32 { if x > 0 { x + 1 } else { 0 } }\n";
        let base = SourceFile::new(PathBuf::from("sample.rs"), target.to_string());
        let expanded = SourceFile::new(
            PathBuf::from("sample.rs"),
            format!(
                "{target}{}",
                (0..20)
                    .map(|index| format!("fn helper_{index}() -> i32 {{ {index} }}\n"))
                    .collect::<String>()
            ),
        );

        let base_target = metrics_source(&base)
            .expect("base metrics")
            .into_iter()
            .find(|region| region.name == "target")
            .expect("base target");
        let expanded_target = metrics_source(&expanded)
            .expect("expanded metrics")
            .into_iter()
            .find(|region| region.name == "target")
            .expect("expanded target");

        assert_eq!(
            serde_json::to_value((
                base_target.complexity,
                base_target.expressivity,
                base_target.halstead,
                base_target.heuristic_burden,
            ))
            .expect("base intrinsic metrics"),
            serde_json::to_value((
                expanded_target.complexity,
                expanded_target.expressivity,
                expanded_target.halstead,
                expanded_target.heuristic_burden,
            ))
            .expect("expanded intrinsic metrics")
        );
    }

    #[test]
    fn nested_class_and_method_regions_are_both_scored() {
        let source = SourceFile::new(
            PathBuf::from("sample.js"),
            "class Worker { run(value) { if (value) { return value; } return 0; } }\n".to_string(),
        );
        let report = metrics_source(&source).expect("metrics");
        assert!(
            report
                .iter()
                .any(|region| region.kind == "class_declaration")
        );
        assert!(
            report
                .iter()
                .any(|region| region.kind == "method_definition")
        );
        assert!(
            report
                .iter()
                .all(|region| (0.0..=1.0).contains(&region.heuristic_burden.score))
        );

        let python = SourceFile::new(
            PathBuf::from("sample.py"),
            "class Worker:\n    def run(self, value):\n        if value:\n            return value\n        return 0\n"
                .to_string(),
        );
        let python_report = metrics_source(&python).expect("python metrics");
        assert!(
            python_report
                .iter()
                .any(|region| region.kind == "class_definition")
        );
        assert!(
            python_report
                .iter()
                .any(|region| region.kind == "function_definition")
        );
    }

    #[test]
    fn python_metrics_keep_async_decorated_and_nested_callable_regions() {
        let source = SourceFile::new(
            PathBuf::from("behavioral.py"),
            include_str!("../../../tests/fixtures/python/behavioral.py").to_string(),
        );
        let report = metrics_source(&source).expect("Python metrics");
        let expected = [
            ("traced", 4, 9),
            ("wrapper", 5, 7),
            ("Service", 12, 18),
            ("process", 13, 18),
            ("normalize", 15, 16),
        ];

        for (name, start_line, end_line) in expected {
            let region = report
                .iter()
                .find(|region| region.name == name)
                .unwrap_or_else(|| panic!("missing Python metric region {name}"));
            assert_eq!(region.span.start_line, start_line, "{name}");
            assert_eq!(region.span.end_line, end_line, "{name}");
        }
        assert!(report.iter().all(|region| region.kind != "file"));
    }

    #[test]
    fn typed_typescript_and_tsx_functions_keep_dialect_regions() {
        let cases = [
            (
                "typed.ts",
                Lang::TypeScript,
                include_str!("../../../tests/fixtures/typescript/typed.ts"),
                "convert",
                13,
                15,
            ),
            (
                "component.tsx",
                Lang::TypeScript,
                include_str!("../../../tests/fixtures/typescript/component.tsx"),
                "View",
                11,
                21,
            ),
            (
                "component.jsx",
                Lang::JavaScript,
                include_str!("../../../tests/fixtures/typescript/component.jsx"),
                "JsxView",
                1,
                10,
            ),
        ];

        for (path, lang, text, name, start_line, end_line) in cases {
            let source = SourceFile::new(PathBuf::from(path), text.to_string());
            let report = metrics_source(&source).expect("typed metrics");
            assert_eq!(source.lang, lang);
            let region = report
                .iter()
                .find(|region| {
                    region.lang == lang
                        && region.name == name
                        && region.kind == "function_declaration"
                })
                .unwrap_or_else(|| panic!("missing {name} in {path}: {report:#?}"));
            assert_eq!(region.span.start_line, start_line);
            assert_eq!(region.span.end_line, end_line);
            assert!(!report.iter().any(|region| region.kind == "file"));
        }
    }

    #[test]
    fn clojure_metrics_skip_nested_call_lists() {
        let source = SourceFile::new(
            PathBuf::from("sample.clj"),
            "(defn square-plus [x] (+ x (* x x)))\n".to_string(),
        );
        let report = metrics_source(&source).expect("metrics");
        assert_eq!(report.len(), 1);
    }

    #[test]
    fn clojure_complexity_counts_control_forms_not_call_lists_or_reader_data() {
        let source = SourceFile::new(
            PathBuf::from("control_edges.clj"),
            include_str!("../../../tests/fixtures/clojure/control_edges.clj").to_string(),
        );
        let report = metrics_source(&source).expect("Clojure metrics");

        let expected = [
            (3, 3.0, 3.0, 2),
            (9, 1.0, 0.0, 0),
            (13, 1.0, 0.0, 0),
            (17, 2.0, 1.0, 1),
            (25, 3.0, 4.0, 2),
        ];
        for (start_line, cyclomatic, cognitive, max_nesting) in expected {
            let region = report
                .iter()
                .find(|region| region.span.start_line == start_line)
                .unwrap_or_else(|| panic!("missing Clojure region at line {start_line}"));
            assert_eq!(
                region.complexity.cyclomatic, cyclomatic,
                "line {start_line}"
            );
            assert_eq!(region.complexity.cognitive, cognitive, "line {start_line}");
            assert_eq!(
                region.complexity.max_nesting, max_nesting,
                "line {start_line}"
            );
        }
    }

    #[test]
    fn complexity_entropy_interaction_has_convergent_ordering() {
        let low_complexity = ComplexityMetrics {
            cyclomatic: 1.0,
            cognitive: 0.0,
            max_nesting: 0,
            nloc: 12,
            maintainability_index: 90.0,
        };
        let high_complexity = ComplexityMetrics {
            cyclomatic: 10.0,
            cognitive: 20.0,
            max_nesting: 4,
            ..low_complexity
        };
        let balanced_information = burden_test_expressivity(0.90, 0.50, 256.0);
        let difficult_information = burden_test_expressivity(0.20, 0.90, 1024.0);

        let baseline = heuristic_burden_metrics(&low_complexity, &balanced_information, 128, true);
        let complexity_only =
            heuristic_burden_metrics(&high_complexity, &balanced_information, 128, true);
        let entropy_only =
            heuristic_burden_metrics(&low_complexity, &difficult_information, 128, true);
        let combined =
            heuristic_burden_metrics(&high_complexity, &difficult_information, 128, true);

        assert_close(baseline.score, 0.069_688_888_888_888_88);
        assert_close(complexity_only.score, 0.374_952_331_154_684_07);
        assert_close(entropy_only.score, 0.184_177_777_777_777_77);
        assert_close(combined.score, 0.539_477_159_041_394_4);

        assert!(baseline.score < entropy_only.score);
        assert!(entropy_only.score < complexity_only.score);
        assert!(complexity_only.score < combined.score);
        assert!(combined.interaction_burden > complexity_only.interaction_burden);
        for score in [baseline, complexity_only, entropy_only, combined] {
            assert!((0.0..=1.0).contains(&score.score));
            assert!((0.0..=0.95).contains(&score.measurement_support));
        }
    }

    #[test]
    fn size_increases_support_without_claiming_refactor_confidence() {
        let complexity = ComplexityMetrics {
            cyclomatic: 4.0,
            cognitive: 6.0,
            max_nesting: 2,
            nloc: 8,
            maintainability_index: 70.0,
        };
        let mut small = burden_test_expressivity(0.85, 0.70, 256.0);
        small.tokens = 8;
        let mut large = small;
        large.tokens = 256;
        let small_score = heuristic_burden_metrics(&complexity, &small, 16, true);
        let large_score = heuristic_burden_metrics(&complexity, &large, 256, true);
        assert!(large_score.size_support > small_score.size_support);
        assert!(large_score.measurement_support > small_score.measurement_support);
        assert!(large_score.score > small_score.score);

        let simple_large = heuristic_burden_metrics(
            &ComplexityMetrics {
                cyclomatic: 1.0,
                cognitive: 0.0,
                max_nesting: 0,
                nloc: 80,
                maintainability_index: 90.0,
            },
            &burden_test_expressivity(0.95, 0.50, 256.0),
            256,
            true,
        );
        assert!(simple_large.score < large_score.score);
    }

    #[test]
    fn burden_normalization_surfaces_outlier_but_not_flat_or_tied_values() {
        let mut outlier_values = vec![0.10; 9];
        outlier_values.push(0.30);
        let (distribution, normalized) = burden_normalization(&outlier_values);
        assert_close(distribution.mean, 0.12);
        assert_close(distribution.median, 0.10);
        assert_close(distribution.stddev, 0.06);
        assert_close(distribution.p25, 0.10);
        assert_close(distribution.p75, 0.10);
        assert_close(distribution.min, 0.10);
        assert_close(distribution.max, 0.30);
        assert!(!distribution.flat);
        assert!(distribution.relative_outlier_eligible);
        assert_close(normalized[9].0, 3.0);
        assert_close(normalized[9].1, 1.0);

        let mut relative_outlier = heuristic_burden_metrics(
            &ComplexityMetrics {
                cyclomatic: 1.0,
                cognitive: 0.0,
                max_nesting: 0,
                nloc: 12,
                maintainability_index: 90.0,
            },
            &burden_test_expressivity(0.90, 0.50, 256.0),
            128,
            true,
        );
        relative_outlier.score = outlier_values[9];
        relative_outlier.repo_relative = Some(RepoRelativeBurden {
            zscore: normalized[9].0,
            percentile: normalized[9].1,
        });
        assert!(is_heuristic_outlier(relative_outlier, distribution));

        let (flat, flat_normalized) = burden_normalization(&[0.20; 10]);
        assert!(flat.flat);
        assert!(!flat.relative_outlier_eligible);
        assert!(
            flat_normalized
                .iter()
                .all(|(zscore, percentile)| *zscore == 0.0 && *percentile == 0.5)
        );
        let mut tied = relative_outlier;
        tied.score = 0.20;
        tied.repo_relative = Some(RepoRelativeBurden {
            zscore: flat_normalized[0].0,
            percentile: flat_normalized[0].1,
        });
        assert!(!is_heuristic_outlier(tied, flat));

        let (exact, _) = burden_normalization(&[0.10, 0.20, 0.30, 0.40]);
        assert_eq!(exact.count, 4);
        assert!(!exact.relative_outlier_eligible);
        assert_close(exact.mean, 0.25);
        assert_close(exact.median, 0.25);
        assert_close(exact.stddev, 0.111_803_398_874_989_48);
        assert_close(exact.p25, 0.175);
        assert_close(exact.p75, 0.325);
        let mut high_absolute_burden = relative_outlier;
        high_absolute_burden.score = 1.0;
        high_absolute_burden.repo_relative = Some(RepoRelativeBurden {
            zscore: 10.0,
            percentile: 1.0,
        });
        assert!(!is_heuristic_outlier(high_absolute_burden, exact));
    }

    #[test]
    fn heuristic_burden_json_has_no_readability_health_or_confidence_claims() {
        let burden = heuristic_burden_metrics(
            &ComplexityMetrics {
                cyclomatic: 4.0,
                cognitive: 6.0,
                max_nesting: 2,
                nloc: 12,
                maintainability_index: 70.0,
            },
            &burden_test_expressivity(0.90, 0.50, 256.0),
            128,
            true,
        );
        let json = serde_json::to_value(burden).expect("serialize burden");
        assert!(json["score"].is_number());
        assert!(json["measurement_support"].is_number());
        assert_eq!(json["basis"], HEURISTIC_BASIS);
        assert!(json["repo_relative"].is_null());
        for forbidden in [
            "readability",
            "health",
            "refactor_confidence",
            "refactor_confidence_score",
            "measurement_confidence",
            "confidence_basis",
        ] {
            assert!(json.get(forbidden).is_none(), "unexpected key {forbidden}");
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-12,
            "expected {expected}, got {actual}"
        );
    }

    fn burden_test_expressivity(
        token_entropy: f64,
        structural_entropy: f64,
        information_volume: f64,
    ) -> ExpressivityMetrics {
        ExpressivityMetrics {
            tokens: 128,
            vocabulary: 64,
            decision_density: 0.0,
            unique_token_ratio: 0.5,
            comment_to_code_ratio: 0.0,
            byte_entropy_bits_per_byte: 4.0,
            token_entropy,
            structural_entropy,
            information_volume,
        }
    }

    struct MetricTestPack;

    impl LangPack for MetricTestPack {
        fn name(&self) -> &'static str {
            "metric-test"
        }

        fn lang(&self) -> Lang {
            Lang::Generic
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["metric"]
        }

        fn grammar(&self) -> Option<tree_sitter::Language> {
            None
        }

        fn line_comments(&self) -> &'static [&'static str] {
            &["#"]
        }

        fn metrics_regions(&self) -> &'static [&'static str] {
            &["region"]
        }

        fn metrics_branches(&self) -> &'static [&'static str] {
            &["branch"]
        }

        fn metrics_nesting(&self) -> &'static [&'static str] {
            &["branch"]
        }

        fn metrics_flow_breaks(&self) -> &'static [&'static str] {
            &["flow"]
        }

        fn halstead_operator_tokens(&self) -> &'static [&'static str] {
            &["op"]
        }

        fn enclosing_region(
            &self,
            _node: tree_sitter::Node<'_>,
            _text: &str,
        ) -> Option<RegionSpan> {
            None
        }
    }

    #[test]
    fn test_pack_metric_declarations_drive_halstead_without_core_edits() {
        let pack = MetricTestPack;
        let halstead = halstead_for_text(&pack, "a op b op c");
        assert_eq!(halstead.distinct_operators, 1);
        assert_eq!(halstead.total_operators, 2);
        assert_eq!(halstead.distinct_operands, 3);
    }
}
