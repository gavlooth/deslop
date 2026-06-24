use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use deslop_core::{Lang, Span};
use deslop_lang::{LangPack, RegionSpan, Registry};
use deslop_parse::{SourceFile, parse_tree};
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
    pub functions: Vec<RegionMetrics>,
    pub hotspots: Vec<Hotspot>,
    pub health_score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionMetrics {
    pub path: PathBuf,
    pub lang: Lang,
    pub name: String,
    pub span: Span,
    pub complexity: ComplexityMetrics,
    pub expressivity: ExpressivityMetrics,
    pub halstead: HalsteadMetrics,
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
    pub compression_ratio: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct HalsteadMetrics {
    pub distinct_operators: usize,
    pub distinct_operands: usize,
    pub total_operators: usize,
    pub total_operands: usize,
    pub volume: f64,
    pub difficulty: f64,
    pub effort: f64,
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

#[derive(Debug, Clone)]
struct Token {
    text: String,
    is_comment: bool,
}

pub fn metrics_paths(paths: &[PathBuf], config: MetricsConfig) -> Result<MetricsReport> {
    let mut functions = Vec::new();
    for path in input_files(paths)? {
        let source = SourceFile::read(&path)?;
        functions.extend(metrics_source(&source)?);
    }
    functions.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.span.start_line.cmp(&b.span.start_line))
            .then(a.name.cmp(&b.name))
    });
    let hotspots = detect_hotspots(&functions, config.sigma);
    let health_score = health_score(&functions, &hotspots);
    Ok(MetricsReport {
        schema: "deslop.metrics/1",
        functions,
        hotspots,
        health_score,
    })
}

pub fn metrics_source(source: &SourceFile) -> Result<Vec<RegionMetrics>> {
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
    out.push_str(&metrics_summary_line(report));
    if !hotspots_only {
        out.push_str(&regions_text(&report.functions));
    }
    out.push_str(&hotspots_text(&report.hotspots));
    out
}

fn metrics_summary_line(report: &MetricsReport) -> String {
    format!(
        "Repo health: {:.1}/100 ({} region(s), {} hotspot(s))\n",
        report.health_score,
        report.functions.len(),
        report.hotspots.len()
    )
}

fn regions_text(functions: &[RegionMetrics]) -> String {
    let mut out = String::from(
        "\nregion                                      cyc cog nest nloc   MI  dens uniq  compr\n",
    );
    for region in functions {
        out.push_str(&region_text_line(region));
    }
    out
}

fn region_text_line(region: &RegionMetrics) -> String {
    format!(
        "{:<43} {:>3.0} {:>3.0} {:>4} {:>4} {:>5.1} {:>5.3} {:>4.2} {:>6.3}\n",
        short_name(region),
        region.complexity.cyclomatic,
        region.complexity.cognitive,
        region.complexity.max_nesting,
        region.complexity.nloc,
        region.complexity.maintainability_index,
        region.expressivity.decision_density,
        region.expressivity.unique_token_ratio,
        region.expressivity.compression_ratio,
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
    let Some(tree) = parse_tree(source.lang, &source.text)? else {
        return Ok(vec![whole_file_region(source)]);
    };
    if tree.root_node().has_error() || pack.metrics_regions().is_empty() {
        return Ok(vec![whole_file_region(source)]);
    }
    let mut regions = Vec::new();
    collect_regions(
        tree.root_node(),
        pack.metrics_regions(),
        &source.text,
        &mut regions,
    );
    if regions.is_empty() {
        regions.push(whole_file_region(source));
    }
    Ok(regions)
}

fn collect_regions(
    node: Node<'_>,
    region_kinds: &[&str],
    text: &str,
    regions: &mut Vec<MetricRegion>,
) {
    if region_kinds.contains(&node.kind()) {
        regions.push(MetricRegion {
            name: region_name(node, text),
            span: region_from_node(node, text),
            node: Some(NodeRange {
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
            }),
        });
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_regions(child, region_kinds, text, regions);
    }
}

fn whole_file_region(source: &SourceFile) -> MetricRegion {
    let end_line = source.lines().len().max(1);
    MetricRegion {
        name: "file".to_string(),
        span: RegionSpan {
            start_line: 1,
            end_line,
            start_byte: 0,
            end_byte: source.text.len(),
        },
        node: None,
    }
}

fn measure_region(pack: &dyn LangPack, source: &SourceFile, region: MetricRegion) -> RegionMetrics {
    let text = source
        .text
        .get(region.span.start_byte..region.span.end_byte)
        .unwrap_or("");
    let tokens = tokenize(text, pack.line_comments());
    let halstead = halstead(&tokens, pack);
    let ast = ast_stats_for_region(pack, source, region.node);
    let nloc = nloc(text, pack.line_comments());
    let cyclomatic = ast.branch_count as f64 + 1.0;
    let maintainability_index = maintainability_index(halstead.volume, cyclomatic, nloc);
    let expressivity = expressivity(text, &tokens, cyclomatic, nloc, pack.line_comments());
    RegionMetrics {
        path: source.path.clone(),
        lang: source.lang,
        name: region.name,
        span: span_from_region(region.span),
        complexity: complexity_metrics(ast, cyclomatic, nloc, maintainability_index),
        expressivity,
        halstead,
    }
}

fn ast_stats_for_region(
    pack: &dyn LangPack,
    source: &SourceFile,
    node: Option<NodeRange>,
) -> AstStats {
    node.and_then(|range| {
        parse_tree(source.lang, &source.text)
            .ok()
            .flatten()
            .and_then(|tree| {
                tree.root_node()
                    .descendant_for_byte_range(range.start_byte, range.end_byte)
                    .map(|node| ast_complexity(node, pack))
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

fn ast_complexity(node: Node<'_>, pack: &dyn LangPack) -> AstStats {
    fn visit(node: Node<'_>, pack: &dyn LangPack, nesting: usize, stats: &mut AstStats) {
        let kind = node.kind();
        let is_branch = pack.metrics_branches().contains(&kind);
        let is_nesting = pack.metrics_nesting().contains(&kind);
        if is_branch {
            stats.branch_count += 1;
            stats.cognitive += 1 + nesting;
        }
        if pack.metrics_flow_breaks().contains(&kind) {
            stats.cognitive += 1;
        }
        let next_nesting = if is_nesting { nesting + 1 } else { nesting };
        stats.max_nesting = stats.max_nesting.max(next_nesting);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            visit(child, pack, next_nesting, stats);
        }
    }
    let mut stats = AstStats::default();
    visit(node, pack, 0, &mut stats);
    stats
}

#[derive(Debug, Clone)]
struct MetricRegion {
    name: String,
    span: RegionSpan,
    node: Option<NodeRange>,
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
        effort: volume * difficulty,
    }
}

fn expressivity(
    text: &str,
    tokens: &[Token],
    cyclomatic: f64,
    nloc: usize,
    comment_tokens: &[&str],
) -> ExpressivityMetrics {
    let code_tokens: Vec<_> = tokens.iter().filter(|token| !token.is_comment).collect();
    let vocabulary = code_tokens
        .iter()
        .map(|token| token.text.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let tokens_len = code_tokens.len();
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
        tokens: tokens_len,
        vocabulary,
        decision_density: ratio(cyclomatic, tokens_len),
        unique_token_ratio: ratio(vocabulary as f64, tokens_len),
        comment_to_code_ratio: ratio(comment_lines as f64, nloc),
        compression_ratio: entropy_ratio(text),
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
            "halstead-effort",
            region.halstead.effort,
            distributions.effort,
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
            (
                "compression-ratio",
                region.expressivity.compression_ratio,
                distributions.compression_ratio,
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
    effort: Distribution,
    decision_density: Distribution,
    unique_token_ratio: Distribution,
    compression_ratio: Distribution,
    comment_to_code_ratio: Distribution,
}

impl MetricDistributions {
    fn new(functions: &[RegionMetrics]) -> Self {
        Self {
            cyclomatic: distribution(functions.iter().map(|region| region.complexity.cyclomatic)),
            cognitive: distribution(functions.iter().map(|region| region.complexity.cognitive)),
            nloc: distribution(functions.iter().map(|region| region.complexity.nloc as f64)),
            effort: distribution(functions.iter().map(|region| region.halstead.effort)),
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
            compression_ratio: distribution(
                functions
                    .iter()
                    .map(|region| region.expressivity.compression_ratio),
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

fn health_score(functions: &[RegionMetrics], hotspots: &[Hotspot]) -> f64 {
    if functions.is_empty() {
        return 100.0;
    }
    let avg_mi = functions
        .iter()
        .map(|region| region.complexity.maintainability_index)
        .sum::<f64>()
        / functions.len() as f64;
    let hotspot_ratio = hotspots.len() as f64 / functions.len() as f64;
    (avg_mi - (hotspot_ratio * 100.0)).clamp(0.0, 100.0)
}

fn entropy_ratio(text: &str) -> f64 {
    if text.is_empty() {
        return 1.0;
    }
    let mut counts = BTreeMap::new();
    for byte in text.bytes() {
        *counts.entry(byte).or_insert(0usize) += 1;
    }
    let len = text.len() as f64;
    let entropy = counts
        .values()
        .map(|count| {
            let probability = *count as f64 / len;
            -probability * probability.log2()
        })
        .sum::<f64>();
    (entropy / 8.0).clamp(0.0, 1.0)
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
    fn halstead_known_numbers() {
        let halstead = halstead_for_text(&RUST_PACK, "a + b * c");
        assert_eq!(halstead.distinct_operators, 2);
        assert_eq!(halstead.total_operators, 2);
        assert_eq!(halstead.distinct_operands, 3);
        assert_eq!(halstead.total_operands, 3);
        assert!((halstead.volume - 11.609_640).abs() < 0.000_01);
        assert!((halstead.difficulty - 1.0).abs() < 0.000_01);
        assert!((halstead.effort - 11.609_640).abs() < 0.000_01);
    }

    #[test]
    fn hotspot_detection_flags_bloated_outlier_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("sample.rs");
        std::fs::write(
            &path,
            "fn clean_a() -> i32 { 1 }\nfn clean_b() -> i32 { 2 }\nfn clean_c() -> i32 { 3 }\nfn bloated(x: i32) -> i32 {\n  let data = x + x + x + x + x + x + x + x;\n  if data > 0 { if data > 1 { if data > 2 { if data > 3 { return data; } } } }\n  data\n}\n",
        )
        .expect("fixture");
        let report = metrics_paths(&[path], MetricsConfig { sigma: 1.0 }).expect("metrics");
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
