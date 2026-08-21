use std::cmp::Ordering;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::Deserialize;

const RUN_SCHEMA: &str = "criv.performance-run.v2";
const SUMMARY_SCHEMA: &str = "criv.performance-summary.v2";
const SAMPLE_SCHEMA: &str = "criv.performance-sample.v2";
const NOTE_SCHEMA: &str = "criv.performance-git-note.v2";

#[derive(Debug, Parser)]
#[command(
    name = "criv-perf-report",
    about = "Render validated criv performance evidence as self-contained HTML"
)]
struct Args {
    /// Completed performance result directory containing run.json and summary.json.
    #[arg(long)]
    result_dir: PathBuf,
    /// Published-note candidate containing workflow and artifact identity.
    #[arg(long)]
    note: PathBuf,
    /// Self-contained HTML report destination.
    #[arg(long)]
    output: PathBuf,
    /// Optional Markdown summary suitable for GITHUB_STEP_SUMMARY.
    #[arg(long)]
    github_summary: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct RunIdentity {
    schema: String,
    run_id: String,
    started_at_utc: String,
    revision: String,
    dirty: bool,
    binary_digest: String,
    profile: String,
    samples: usize,
    machine: MachineIdentity,
    manifests: Vec<ManifestIdentity>,
    cases: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MachineIdentity {
    os: String,
    release: String,
    architecture: String,
    processor: String,
    rustc_verbose: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
struct ManifestIdentity {
    id: String,
    tier: String,
    digest: String,
    observed_repository: String,
    observed_revision: String,
    source_files: usize,
    source_bytes: usize,
    #[serde(default)]
    relationships: usize,
}

#[derive(Debug, Deserialize)]
struct SummaryDocument {
    schema: String,
    run: RunIdentity,
    cases: Vec<CaseSummary>,
}

#[derive(Debug, Deserialize)]
struct CaseSummary {
    workload: String,
    workload_digest: String,
    case: String,
    cache_state: String,
    successful_samples: usize,
    failed_samples: usize,
    selected_source_files: usize,
    selected_source_bytes: usize,
    selected_elixir_files: usize,
    selected_elixir_bytes: u64,
    parsed_elixir_files: usize,
    parsed_elixir_bytes: u64,
    #[serde(default)]
    expected_relationships: usize,
    #[serde(default)]
    parsed_relationships: usize,
    #[serde(default)]
    published_relationships: Option<usize>,
    elixir_path_digest: Option<String>,
    real_seconds: Option<MetricSummary>,
    user_seconds: Option<MetricSummary>,
    system_seconds: Option<MetricSummary>,
    peak_rss_bytes: Option<MetricSummary>,
}

#[derive(Debug, Deserialize)]
struct MetricSummary {
    minimum: f64,
    median: f64,
    maximum: f64,
    median_absolute_deviation: f64,
}

#[derive(Debug, Deserialize)]
struct SampleIdentity {
    schema: String,
    run_id: String,
    exit_status: i32,
}

#[derive(Debug, Deserialize)]
struct PerformanceNote {
    schema: String,
    commit: String,
    pushed_ref: String,
    workflow: WorkflowIdentity,
    artifact: String,
    evidence: NoteEvidence,
}

#[derive(Debug, Deserialize)]
struct WorkflowIdentity {
    url: String,
    run_id: String,
    attempt: String,
}

#[derive(Debug, Deserialize)]
struct NoteEvidence {
    observation: String,
    run_id: String,
    samples: usize,
}

struct Evidence {
    run: RunIdentity,
    cases: Vec<CaseSummary>,
    note: PerformanceNote,
    raw_samples: usize,
    failed_samples: usize,
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-perf-report: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    let evidence = load_evidence(&args.result_dir, &args.note)?;
    write_output(&args.output, &render_html(&evidence))?;
    if let Some(path) = args.github_summary {
        write_output(&path, &render_github_summary(&evidence))?;
    }
    Ok(())
}

fn load_evidence(result_dir: &Path, note_path: &Path) -> Result<Evidence, String> {
    let run: RunIdentity = read_json(&result_dir.join("run.json"))?;
    let summary: SummaryDocument = read_json(&result_dir.join("summary.json"))?;
    let note: PerformanceNote = read_json(note_path)?;
    let samples_path = result_dir.join("samples.jsonl");
    let samples_text = fs::read_to_string(&samples_path)
        .map_err(|error| format!("failed to read {}: {error}", samples_path.display()))?;
    let samples = samples_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str::<SampleIdentity>(line).map_err(|error| {
                format!(
                    "failed to parse {} line {}: {error}",
                    samples_path.display(),
                    index + 1
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    if run.schema != RUN_SCHEMA || summary.schema != SUMMARY_SCHEMA || note.schema != NOTE_SCHEMA {
        return Err("unsupported performance evidence schema".into());
    }
    if summary.run.schema != RUN_SCHEMA
        || summary.run.run_id != run.run_id
        || note.evidence.run_id != run.run_id
    {
        return Err("performance run identities do not match".into());
    }
    if note.commit != run.revision {
        return Err(format!(
            "note commit {} does not match measured revision {}",
            note.commit, run.revision
        ));
    }
    if run.dirty || run.profile != "release" || note.evidence.observation != "external-subprocess" {
        return Err("report requires clean release evidence from external subprocesses".into());
    }
    if note.evidence.samples != run.samples {
        return Err("note and run sample counts do not match".into());
    }
    if samples
        .iter()
        .any(|sample| sample.schema != SAMPLE_SCHEMA || sample.run_id != run.run_id)
    {
        return Err("raw samples contain a foreign run or schema".into());
    }
    let summarized_samples = summary
        .cases
        .iter()
        .map(|case| case.successful_samples + case.failed_samples)
        .sum::<usize>();
    if summarized_samples != samples.len() {
        return Err(format!(
            "summary accounts for {summarized_samples} samples but raw evidence contains {}",
            samples.len()
        ));
    }
    if summary.cases.iter().any(|case| case.real_seconds.is_none()) {
        return Err("every report row requires successful elapsed-time evidence".into());
    }
    if summary.cases.iter().any(|case| {
        !run.manifests
            .iter()
            .any(|manifest| manifest.id == case.workload && manifest.digest == case.workload_digest)
    }) {
        return Err("timing summaries contain an unknown workload identity".into());
    }
    for case in &summary.cases {
        let manifest = run
            .manifests
            .iter()
            .find(|manifest| manifest.id == case.workload)
            .unwrap();
        let no_source_selected = case.selected_source_files == 0 && case.selected_source_bytes == 0;
        let complete_source_selected = case.selected_source_files == manifest.source_files
            && case.selected_source_bytes == manifest.source_bytes;
        if !no_source_selected && !complete_source_selected {
            return Err("timing summaries do not match the manifest source shape".into());
        }
        if case.selected_elixir_files != case.parsed_elixir_files
            || case.selected_elixir_bytes != case.parsed_elixir_bytes
            || (case.selected_elixir_files > 0) != case.elixir_path_digest.is_some()
        {
            return Err("timing summaries have incomplete Elixir parse coverage".into());
        }
        if complete_source_selected
            && (case.expected_relationships != manifest.relationships
                || case.parsed_relationships != manifest.relationships)
        {
            return Err("timing summaries do not match the manifest relationship shape".into());
        }
        if case
            .published_relationships
            .is_some_and(|count| count != manifest.relationships)
        {
            return Err("timing summaries have incomplete State relationship coverage".into());
        }
    }
    let failed_samples = samples
        .iter()
        .filter(|sample| sample.exit_status != 0)
        .count();

    Ok(Evidence {
        run,
        cases: summary.cases,
        note,
        raw_samples: samples.len(),
        failed_samples,
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn write_output(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::write(path, contents)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn render_html(evidence: &Evidence) -> String {
    let successful = evidence.raw_samples - evidence.failed_samples;
    let global_max = evidence
        .cases
        .iter()
        .filter_map(|case| case.real_seconds.as_ref())
        .map(|metric| metric.maximum)
        .fold(0.0_f64, f64::max)
        .max(0.001);
    let focal = evidence
        .cases
        .iter()
        .filter_map(|case| {
            case.real_seconds
                .as_ref()
                .map(|metric| ((case.workload.as_str(), case.case.as_str()), metric.median))
        })
        .max_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal))
        .map(|(identity, _)| identity);
    let short_commit = evidence
        .note
        .commit
        .get(..8)
        .unwrap_or(&evidence.note.commit);
    let machine_label = format!(
        "{} {} · {}",
        evidence.run.machine.os, evidence.run.machine.release, evidence.run.machine.architecture
    );

    let mut html = String::with_capacity(48_000);
    html.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<title>criv performance report · ");
    html.push_str(&escape_html(short_commit));
    html.push_str("</title>\n<style>\n");
    html.push_str(STYLES);
    html.push_str("\n</style>\n</head>\n<body>\n<main>\n<header class=\"report-header\">\n");
    html.push_str("<p class=\"eyebrow\">External release-binary observation</p>\n<h1>criv performance report</h1>\n");
    let _ = writeln!(
        html,
        "<p class=\"lede\"><strong>{successful}/{}</strong> samples succeeded across <strong>{}</strong> workloads and <strong>{}</strong> command cases.</p>",
        evidence.raw_samples,
        evidence.run.manifests.len(),
        evidence.run.cases.len()
    );
    html.push_str("<div class=\"runline\">");
    let _ = write!(
        html,
        "<span><b>Commit</b> <code>{}</code></span><span><b>Started</b> {}</span><span><b>Profile</b> {}</span>",
        escape_html(short_commit),
        escape_html(&evidence.run.started_at_utc),
        escape_html(&evidence.run.profile)
    );
    html.push_str("</div>\n");
    let _ = writeln!(
        html,
        "<p class=\"actions\"><a href=\"{}\">Open workflow run {}</a> <span aria-hidden=\"true\">·</span> Artifact <code>{}</code></p>",
        escape_html(&evidence.note.workflow.url),
        escape_html(&evidence.note.workflow.run_id),
        escape_html(&evidence.note.artifact)
    );
    html.push_str("</header>\n<section aria-labelledby=\"comparison-title\">\n<h2 id=\"comparison-title\">Elapsed time by command</h2>\n");
    let _ = writeln!(
        html,
        "<p>Shared scale: 0 to {}. Lines show minimum to maximum, dark segments show median ± MAD, and dots mark medians. The slowest median is accented.</p>",
        format_duration(global_max)
    );
    html.push_str("<div class=\"plots\">\n");
    for manifest in &evidence.run.manifests {
        let mut cases = evidence
            .cases
            .iter()
            .filter(|case| case.workload == manifest.id)
            .collect::<Vec<_>>();
        cases.sort_by(|left, right| {
            right
                .real_seconds
                .as_ref()
                .unwrap()
                .median
                .partial_cmp(&left.real_seconds.as_ref().unwrap().median)
                .unwrap_or(Ordering::Equal)
        });
        html.push_str(&render_plot(manifest, &cases, global_max, focal));
    }
    html.push_str("</div>\n</section>\n<section aria-labelledby=\"exact-title\">\n<h2 id=\"exact-title\">Exact timing summaries</h2>\n<p>All values are process-level seconds converted for display. MAD is median absolute deviation.</p>\n");
    for manifest in &evidence.run.manifests {
        let mut cases = evidence
            .cases
            .iter()
            .filter(|case| case.workload == manifest.id)
            .collect::<Vec<_>>();
        cases.sort_by(|left, right| {
            right
                .real_seconds
                .as_ref()
                .unwrap()
                .median
                .partial_cmp(&left.real_seconds.as_ref().unwrap().median)
                .unwrap_or(Ordering::Equal)
        });
        let _ = writeln!(
            html,
            "<h3>{} <span>{} tier</span></h3>",
            escape_html(&manifest.id),
            escape_html(&manifest.tier)
        );
        let _ = writeln!(
            html,
            "<div class=\"table-wrap\" role=\"region\" tabindex=\"0\" aria-label=\"Exact timing summary for {}\"><table><thead><tr><th scope=\"col\">Command</th><th scope=\"col\">State</th><th scope=\"col\" class=\"num\">Median</th><th scope=\"col\" class=\"num\">MAD</th><th scope=\"col\" class=\"num\">Min–max</th><th scope=\"col\" class=\"num\">User</th><th scope=\"col\" class=\"num\">System</th><th scope=\"col\" class=\"num\">Peak RSS</th><th scope=\"col\" class=\"num\">Elixir parsed</th><th scope=\"col\" class=\"num\">Relationships</th><th scope=\"col\" class=\"num\">Samples</th></tr></thead><tbody>",
            escape_html(&manifest.id)
        );
        for case in cases {
            let real = case.real_seconds.as_ref().unwrap();
            let is_focal = focal == Some((case.workload.as_str(), case.case.as_str()));
            let _ = writeln!(
                html,
                "<tr{}><th scope=\"row\"><code>{}</code></th><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}–{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}/{} files<br>{}/{} bytes</td><td class=\"num\">{}/{} parsed<br>{} published</td><td class=\"num\">{}/{}</td></tr>",
                if is_focal { " class=\"focal-row\"" } else { "" },
                escape_html(&case.case),
                escape_html(&case.cache_state),
                format_duration(real.median),
                format_duration(real.median_absolute_deviation),
                format_duration(real.minimum),
                format_duration(real.maximum),
                case.user_seconds
                    .as_ref()
                    .map_or("n/a".into(), |metric| format_duration(metric.median)),
                case.system_seconds
                    .as_ref()
                    .map_or("n/a".into(), |metric| format_duration(metric.median)),
                case.peak_rss_bytes
                    .as_ref()
                    .map_or("n/a".into(), |metric| format_bytes(metric.median as u64)),
                case.parsed_elixir_files,
                case.selected_elixir_files,
                case.parsed_elixir_bytes,
                case.selected_elixir_bytes,
                case.parsed_relationships,
                case.expected_relationships,
                case.published_relationships
                    .map_or_else(|| "n/a".into(), |count| count.to_string()),
                case.successful_samples,
                case.successful_samples + case.failed_samples
            );
        }
        html.push_str("</tbody></table></div>\n");
    }
    html.push_str("</section>\n<section aria-labelledby=\"identity-title\">\n<h2 id=\"identity-title\">Evidence identity</h2>\n<dl class=\"identity\">\n");
    definition(&mut html, "Run", &evidence.run.run_id);
    definition(&mut html, "Machine", &machine_label);
    definition(&mut html, "Processor", &evidence.run.machine.processor);
    definition(&mut html, "Machine digest", &evidence.run.machine.digest);
    definition(&mut html, "Binary digest", &evidence.run.binary_digest);
    definition(&mut html, "Pushed ref", &evidence.note.pushed_ref);
    definition(
        &mut html,
        "Workflow attempt",
        &evidence.note.workflow.attempt,
    );
    definition(
        &mut html,
        "Rust compiler",
        evidence
            .run
            .machine
            .rustc_verbose
            .lines()
            .next()
            .unwrap_or("unavailable"),
    );
    html.push_str("</dl>\n<h3>Workload provenance</h3>\n<div class=\"table-wrap\" role=\"region\" tabindex=\"0\" aria-label=\"Workload provenance\"><table><thead><tr><th scope=\"col\">Workload</th><th scope=\"col\">Tier</th><th scope=\"col\">Sources</th><th scope=\"col\">Bytes</th><th scope=\"col\">Relationships</th><th scope=\"col\">Observed repository</th><th scope=\"col\">Observed revision</th><th scope=\"col\">Manifest digest</th></tr></thead><tbody>\n");
    for manifest in &evidence.run.manifests {
        let _ = writeln!(
            html,
            "<tr><th scope=\"row\">{}</th><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td>{}</td><td><code>{}</code></td><td><code>{}</code></td></tr>",
            escape_html(&manifest.id),
            escape_html(&manifest.tier),
            manifest.source_files,
            format_bytes(manifest.source_bytes as u64),
            manifest.relationships,
            escape_html(&manifest.observed_repository),
            escape_html(&manifest.observed_revision),
            escape_html(&manifest.digest)
        );
    }
    html.push_str("</tbody></table></div>\n</section>\n<footer>Derived from <code>run.json</code>, <code>summary.json</code>, and <code>samples.jsonl</code>. JSON evidence remains canonical. No performance instrumentation is compiled into criv.</footer>\n</main>\n</body>\n</html>\n");
    html
}

fn render_plot(
    manifest: &ManifestIdentity,
    cases: &[&CaseSummary],
    global_max: f64,
    focal: Option<(&str, &str)>,
) -> String {
    const WIDTH: f64 = 760.0;
    const LEFT: f64 = 220.0;
    const RIGHT: f64 = 110.0;
    const TOP: f64 = 44.0;
    const ROW: f64 = 30.0;
    let height = TOP + ROW * cases.len() as f64 + 24.0;
    let plot_width = WIDTH - LEFT - RIGHT;
    let x = |value: f64| LEFT + (value / global_max).clamp(0.0, 1.0) * plot_width;
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        "<figure><figcaption><strong>{}</strong><span>{} tier · {} cases</span></figcaption><svg role=\"img\" aria-label=\"Elapsed timing ranges for {}\" viewBox=\"0 0 {WIDTH:.0} {height:.0}\" width=\"100%\">",
        escape_html(&manifest.id),
        escape_html(&manifest.tier),
        cases.len(),
        escape_html(&manifest.id)
    );
    let _ = writeln!(
        svg,
        "<line x1=\"{LEFT}\" y1=\"28\" x2=\"{}\" y2=\"28\" class=\"axis\"/><text x=\"{LEFT}\" y=\"19\" class=\"tick\">0 ms</text><text x=\"{}\" y=\"19\" text-anchor=\"end\" class=\"tick\">{}</text>",
        LEFT + plot_width,
        LEFT + plot_width,
        format_duration(global_max)
    );
    for (index, case) in cases.iter().enumerate() {
        let metric = case.real_seconds.as_ref().unwrap();
        let y = TOP + index as f64 * ROW;
        let mad_low = (metric.median - metric.median_absolute_deviation).max(metric.minimum);
        let mad_high = (metric.median + metric.median_absolute_deviation).min(metric.maximum);
        let is_focal = focal == Some((case.workload.as_str(), case.case.as_str()));
        let class = if is_focal { " focal" } else { "" };
        let _ = writeln!(
            svg,
            "<text x=\"{}\" y=\"{}\" text-anchor=\"end\" class=\"case-label\">{}</text><line x1=\"{:.2}\" y1=\"{y:.2}\" x2=\"{:.2}\" y2=\"{y:.2}\" class=\"range\"/><line x1=\"{:.2}\" y1=\"{y:.2}\" x2=\"{:.2}\" y2=\"{y:.2}\" class=\"mad\"/><circle cx=\"{:.2}\" cy=\"{y:.2}\" r=\"4\" class=\"median{class}\"/><text x=\"{}\" y=\"{}\" class=\"plot-value{class}\">{}</text>",
            LEFT - 12.0,
            y + 5.0,
            escape_html(&case.case.replace('_', " ")),
            x(metric.minimum),
            x(metric.maximum),
            x(mad_low),
            x(mad_high),
            x(metric.median),
            LEFT + plot_width + 12.0,
            y + 5.0,
            format_duration(metric.median)
        );
    }
    svg.push_str("</svg><div class=\"mobile-chart\">\n");
    for case in cases {
        let metric = case.real_seconds.as_ref().unwrap();
        let minimum = metric.minimum / global_max * 100.0;
        let maximum = metric.maximum / global_max * 100.0;
        let median = metric.median / global_max * 100.0;
        let mad_low = (metric.median - metric.median_absolute_deviation).max(metric.minimum)
            / global_max
            * 100.0;
        let mad_high = (metric.median + metric.median_absolute_deviation).min(metric.maximum)
            / global_max
            * 100.0;
        let is_focal = focal == Some((case.workload.as_str(), case.case.as_str()));
        let _ = writeln!(
            svg,
            "<div class=\"mobile-row{}\"><span class=\"mobile-case\">{}</span><span class=\"mobile-track\" aria-hidden=\"true\"><i class=\"mobile-range\" style=\"left:{minimum:.3}%;width:{:.3}%\"></i><i class=\"mobile-mad\" style=\"left:{mad_low:.3}%;width:{:.3}%\"></i><i class=\"mobile-dot\" style=\"left:{median:.3}%\"></i></span><span class=\"mobile-value\">{}</span></div>",
            if is_focal { " focal" } else { "" },
            escape_html(&case.case.replace('_', " ")),
            (maximum - minimum).max(0.0),
            (mad_high - mad_low).max(0.0),
            format_duration(metric.median)
        );
    }
    svg.push_str("</div></figure>\n");
    svg
}

fn render_github_summary(evidence: &Evidence) -> String {
    let successful = evidence.raw_samples - evidence.failed_samples;
    let mut output = format!(
        "## Performance report\n\n`{}` · `{}` · {successful}/{} samples succeeded · external subprocess observation\n\n",
        evidence
            .note
            .commit
            .get(..8)
            .unwrap_or(&evidence.note.commit),
        evidence.run.profile,
        evidence.raw_samples
    );
    output.push_str(
        "| Workload | Tier | Slowest median | Fastest median |\n| --- | --- | ---: | ---: |\n",
    );
    for manifest in &evidence.run.manifests {
        let cases = evidence
            .cases
            .iter()
            .filter(|case| case.workload == manifest.id)
            .filter_map(|case| {
                case.real_seconds
                    .as_ref()
                    .map(|metric| (case.case.as_str(), metric.median))
            })
            .collect::<Vec<_>>();
        let slowest = cases
            .iter()
            .max_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal));
        let fastest = cases
            .iter()
            .min_by(|left, right| left.1.partial_cmp(&right.1).unwrap_or(Ordering::Equal));
        if let (Some(slowest), Some(fastest)) = (slowest, fastest) {
            let _ = writeln!(
                output,
                "| `{}` | {} | `{}` {} | `{}` {} |",
                manifest.id,
                manifest.tier,
                slowest.0,
                format_duration(slowest.1),
                fastest.0,
                format_duration(fastest.1)
            );
        }
    }
    output.push_str("\nThe downloadable artifact contains `report.html`, exact timing tables, raw JSONL samples, captured outputs, manifests, and digests. JSON evidence remains canonical.\n");
    output
}

fn definition(output: &mut String, term: &str, value: &str) {
    let _ = writeln!(
        output,
        "<div><dt>{}</dt><dd>{}</dd></div>",
        escape_html(term),
        escape_html(value)
    );
}

fn format_duration(seconds: f64) -> String {
    if seconds >= 1.0 {
        format!("{seconds:.3} s")
    } else if seconds >= 0.1 {
        format!("{:.1} ms", seconds * 1_000.0)
    } else if seconds >= 0.01 {
        format!("{:.2} ms", seconds * 1_000.0)
    } else {
        format!("{:.3} ms", seconds * 1_000.0)
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.2} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.1} KiB", bytes as f64 / 1_024.0)
    } else {
        format!("{bytes} B")
    }
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

const STYLES: &str = r#"
:root {
  color-scheme: light;
  --ink: #1a1a1a;
  --paper: #fafaf7;
  --rule: #d8d4cc;
  --muted: #6b665d;
  --accent: #b3261e;
  --gray-1: #efece4;
  --gray-2: #c8c2b5;
  font-family: Charter, "Iowan Old Style", Georgia, serif;
  font-synthesis: none;
}
* { box-sizing: border-box; }
body { margin: 0; color: var(--ink); background: var(--paper); font-size: 16px; line-height: 1.5; }
main { width: min(1180px, calc(100% - 32px)); margin: 0 auto; padding: 56px 0 40px; }
.report-header { border-top: 5px solid var(--accent); padding-top: 28px; }
.eyebrow { margin: 0 0 6px; color: var(--accent); font: 600 13px/1.2 ui-monospace, "SFMono-Regular", Consolas, monospace; letter-spacing: .05em; text-transform: uppercase; }
h1, h2, h3, p { text-wrap: pretty; }
h1 { margin: 0; max-width: 18ch; font-size: clamp(38px, 7vw, 72px); font-weight: 500; line-height: .98; letter-spacing: -.035em; }
.lede { max-width: 760px; margin: 22px 0 0; font-size: clamp(18px, 2.4vw, 25px); line-height: 1.35; }
.runline { display: flex; flex-wrap: wrap; gap: 8px 24px; margin-top: 24px; color: var(--muted); font-size: 14px; }
.runline b { color: var(--ink); font-weight: 600; }
.actions { margin: 10px 0 0; font-size: 14px; }
a { color: var(--accent); text-decoration-thickness: 1px; text-underline-offset: 3px; }
code, .num { font-family: ui-monospace, "SFMono-Regular", Consolas, monospace; font-variant-numeric: tabular-nums lining-nums; }
section { margin-top: 62px; padding-top: 20px; border-top: 1px solid var(--rule); }
h2 { margin: 0; font-size: clamp(25px, 3vw, 34px); font-weight: 500; letter-spacing: -.015em; }
h2 + p { max-width: 760px; margin: 8px 0 24px; color: var(--muted); }
h3 { margin: 36px 0 10px; font-size: 20px; font-weight: 600; }
h3 span { margin-left: 6px; color: var(--muted); font-size: 14px; font-weight: 400; }
.plots { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 520px), 1fr)); gap: 28px; }
figure { min-width: 0; margin: 0; padding: 0; }
figcaption { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; margin-bottom: 4px; }
figcaption strong { font-size: 18px; font-weight: 600; }
figcaption span { color: var(--muted); font-size: 13px; }
svg { display: block; width: 100%; height: auto; overflow: visible; }
.mobile-chart { display: none; }
.axis { stroke: var(--muted); stroke-width: .65; }
.range { stroke: var(--gray-2); stroke-width: 1.5; }
.mad { stroke: var(--ink); stroke-width: 3; }
.median { fill: var(--ink); }
.median.focal { fill: var(--accent); }
.tick { fill: var(--muted); font: 12px/1 ui-monospace, "SFMono-Regular", Consolas, monospace; }
.case-label { fill: var(--ink); font: 14px/1.2 Charter, "Iowan Old Style", Georgia, serif; }
.plot-value { fill: var(--ink); font: 600 13px/1 ui-monospace, "SFMono-Regular", Consolas, monospace; }
.plot-value.focal { fill: var(--accent); }
.table-wrap { width: 100%; overflow-x: auto; }
table { width: 100%; border-collapse: collapse; font-size: 14px; }
th, td { padding: 9px 10px; border-bottom: 1px solid var(--rule); text-align: left; vertical-align: top; white-space: nowrap; }
thead th { color: var(--muted); font-size: 12px; font-weight: 600; letter-spacing: .035em; }
tbody th { font-weight: 500; }
.num { text-align: right; }
.focal-row > *:first-child { box-shadow: inset 3px 0 0 var(--accent); }
.focal-row .num:first-of-type { color: var(--accent); font-weight: 700; }
.identity { display: grid; grid-template-columns: repeat(auto-fit, minmax(230px, 1fr)); gap: 18px 32px; margin: 26px 0 0; }
.identity div { min-width: 0; }
dt { color: var(--muted); font-size: 12px; font-weight: 600; letter-spacing: .035em; }
dd { margin: 3px 0 0; overflow-wrap: anywhere; font-family: ui-monospace, "SFMono-Regular", Consolas, monospace; font-size: 13px; }
footer { margin-top: 64px; padding-top: 18px; border-top: 1px solid var(--rule); color: var(--muted); font-size: 13px; }
@media (max-width: 640px) {
  main { width: min(100% - 22px, 1180px); padding-top: 28px; }
  section { margin-top: 42px; }
  .plots svg { display: none; }
  .mobile-chart { display: block; margin-top: 12px; }
  .mobile-row { display: grid; grid-template-columns: minmax(118px, 1fr) 92px 74px; gap: 8px; align-items: center; min-height: 28px; font-size: 12px; }
  .mobile-case { line-height: 1.15; }
  .mobile-track { position: relative; height: 18px; }
  .mobile-range, .mobile-mad { position: absolute; top: 8px; min-width: 1px; height: 2px; background: var(--gray-2); }
  .mobile-mad { top: 7px; height: 4px; background: var(--ink); }
  .mobile-dot { position: absolute; top: 5px; width: 7px; height: 7px; margin-left: -3px; border-radius: 50%; background: var(--ink); }
  .mobile-value { text-align: right; font-family: ui-monospace, "SFMono-Regular", Consolas, monospace; font-variant-numeric: tabular-nums; font-weight: 600; }
  .mobile-row.focal .mobile-dot { background: var(--accent); }
  .mobile-row.focal .mobile-value { color: var(--accent); }
}
@media print {
  @page { size: A4 landscape; margin: 12mm; }
  body { background: white; font-size: 10pt; }
  main { width: 100%; padding: 0; }
  .report-header { border-top-width: 3px; }
  h1 { font-size: 34pt; }
  section { break-inside: avoid; margin-top: 28px; }
  .plots { grid-template-columns: 1fr 1fr; gap: 14px; }
  a { color: var(--ink); }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_format_preserves_useful_precision() {
        assert_eq!(format_duration(1.23456), "1.235 s");
        assert_eq!(format_duration(0.23456), "234.6 ms");
        assert_eq!(format_duration(0.023456), "23.46 ms");
        assert_eq!(format_duration(0.0023456), "2.346 ms");
    }

    #[test]
    fn html_escaping_covers_text_and_attributes() {
        assert_eq!(
            escape_html("<bad attr=\"x\">Tom & 'Ada'</bad>"),
            "&lt;bad attr=&quot;x&quot;&gt;Tom &amp; &#39;Ada&#39;&lt;/bad&gt;"
        );
    }
}
