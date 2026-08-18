use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const INPUT_SCHEMA: &str = "criv.discovery-gate-input.v1";
const RECEIPT_SCHEMA: &str = "criv.discovery-release-gate.v1";
const RELEASE_TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
];

#[derive(Debug, Parser)]
#[command(
    name = "criv-discovery-gate",
    about = "Validate the accepted file-discovery release gates"
)]
struct Args {
    /// Gate input that identifies every evidence directory and release artifact.
    #[arg(long)]
    input: PathBuf,
    /// Receipt to write even when one or more gates fail.
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Deserialize)]
struct GateInput {
    schema: String,
    commit: String,
    toolchain: String,
    valid_until_unix: u64,
    primary_target: String,
    live_commands: PathBuf,
    scaling: Vec<ScalingPair>,
    artifacts: ArtifactEvidence,
}

#[derive(Debug, Deserialize)]
struct ScalingPair {
    target: String,
    profile: String,
    selected_files: usize,
    baseline: PathBuf,
    candidate: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ArtifactEvidence {
    normal_dependencies_before: usize,
    normal_dependencies_after: usize,
    native_compiler_or_library_added: bool,
    targets: Vec<ArtifactTarget>,
}

#[derive(Debug, Deserialize)]
struct ArtifactTarget {
    target: String,
    commit: String,
    candidate_binary: PathBuf,
    baseline_binary_digest: String,
    baseline_binary_bytes: u64,
    baseline_build_seconds: Vec<f64>,
    candidate_build_seconds: Vec<f64>,
    clean_builds: bool,
    compiler_cache_disabled: bool,
    registry_inputs_present: bool,
}

#[derive(Debug, Serialize)]
struct GateCheck {
    id: String,
    passed: bool,
    detail: String,
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceDigest {
    label: String,
    run_digest: String,
    summary_digest: String,
    samples_digest: String,
}

#[derive(Debug, Serialize)]
struct ArtifactReceipt {
    target: String,
    path: String,
    digest: String,
    sha256: String,
    file_name: String,
    bytes: u64,
}

#[derive(Debug, Serialize)]
struct GateReceipt {
    schema: &'static str,
    commit: String,
    toolchain: String,
    generated_at_unix: u64,
    valid_until_unix: u64,
    input_digest: String,
    passed: bool,
    evidence: Vec<EvidenceDigest>,
    artifacts: Vec<ArtifactReceipt>,
    checks: Vec<GateCheck>,
}

struct EvidenceDirectory {
    run: Value,
    summary: Value,
    samples: Vec<Value>,
    digests: EvidenceDigest,
}

fn main() {
    if let Err(error) = run(Args::parse()) {
        eprintln!("criv-discovery-gate: {error}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    let input_path = fs::canonicalize(&args.input).map_err(display_error)?;
    let input_bytes = fs::read(&input_path).map_err(display_error)?;
    let input: GateInput = serde_json::from_slice(&input_bytes).map_err(display_error)?;
    validate_input(&input)?;
    let base = input_path.parent().unwrap_or_else(|| Path::new("."));
    let mut checks = Vec::new();
    let mut evidence = Vec::new();

    let live_commands = read_evidence(base, &input.live_commands, "live-commands".into())?;
    gate_live_commands(&live_commands, &mut checks);
    check(
        &mut checks,
        "live-toolchain",
        live_commands.run["rustc_verbose"]
            .as_str()
            .is_some_and(|value| value.contains(&input.toolchain)),
        format!("live rustc identity {}", live_commands.run["rustc_verbose"]),
    );
    evidence.push(live_commands.digests.clone());

    let mut scaling_coverage = BTreeSet::new();
    for pair in &input.scaling {
        let label = format!("{}-{}-{}", pair.target, pair.profile, pair.selected_files);
        let baseline = read_evidence(base, &pair.baseline, format!("baseline-{label}"))?;
        let candidate = read_evidence(base, &pair.candidate, format!("candidate-{label}"))?;
        check(
            &mut checks,
            format!("scaling-{label}-candidate-commit"),
            candidate.run["revision"].as_str() == Some(input.commit.as_str()),
            format!("candidate revision {}", candidate.run["revision"]),
        );
        check(
            &mut checks,
            format!("scaling-{label}-toolchain"),
            candidate.run["rustc_verbose"]
                .as_str()
                .is_some_and(|value| value.contains(&input.toolchain)),
            format!(
                "candidate rustc identity {}",
                candidate.run["rustc_verbose"]
            ),
        );
        gate_scaling(pair, &baseline, &candidate, &mut checks);
        scaling_coverage.insert((
            pair.target.clone(),
            pair.profile.clone(),
            pair.selected_files,
        ));
        evidence.push(baseline.digests);
        evidence.push(candidate.digests);
    }
    gate_scaling_coverage(&input, &scaling_coverage, &mut checks);

    let artifacts = gate_artifacts(base, &input, &mut checks)?;
    let primary_artifact = artifacts
        .iter()
        .find(|artifact| artifact.target == input.primary_target);
    check(
        &mut checks,
        "live-uses-measured-primary-artifact",
        primary_artifact.is_some_and(|artifact| {
            live_commands.run["binary_digest"].as_str() == Some(artifact.digest.as_str())
        }),
        format!(
            "live digest {}, artifact digest {}",
            live_commands.run["binary_digest"],
            primary_artifact.map_or("missing", |artifact| artifact.digest.as_str())
        ),
    );
    let generated_at_unix = now_unix();
    check(
        &mut checks,
        "receipt-not-expired",
        generated_at_unix <= input.valid_until_unix,
        format!(
            "generated {generated_at_unix}, valid until {}",
            input.valid_until_unix
        ),
    );
    check(
        &mut checks,
        "receipt-validity-window",
        input.valid_until_unix <= generated_at_unix.saturating_add(7 * 24 * 60 * 60),
        "receipt validity is at most seven days",
    );
    let passed = checks.iter().all(|item| item.passed);
    let receipt = GateReceipt {
        schema: RECEIPT_SCHEMA,
        commit: input.commit,
        toolchain: input.toolchain,
        generated_at_unix,
        valid_until_unix: input.valid_until_unix,
        input_digest: digest_bytes(&input_bytes),
        passed,
        evidence,
        artifacts,
        checks,
    };
    write_json(&args.output, &receipt)?;
    if !passed {
        return Err(format!(
            "one or more release gates failed; receipt written to {}",
            args.output.display()
        ));
    }
    println!("{}", args.output.display());
    Ok(())
}

fn validate_input(input: &GateInput) -> Result<(), String> {
    if input.schema != INPUT_SCHEMA {
        return Err(format!("unsupported gate input schema {}", input.schema));
    }
    if input.commit.len() != 40 || !input.commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("commit must be one full hexadecimal Git commit ID".into());
    }
    if input.toolchain.trim().is_empty() {
        return Err("toolchain must not be empty".into());
    }
    if !RELEASE_TARGETS.contains(&input.primary_target.as_str()) {
        return Err(format!(
            "primary target is not a release target: {}",
            input.primary_target
        ));
    }
    Ok(())
}

fn read_evidence(base: &Path, path: &Path, label: String) -> Result<EvidenceDirectory, String> {
    let root = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let root = fs::canonicalize(root).map_err(display_error)?;
    let run_bytes = fs::read(root.join("run.json")).map_err(display_error)?;
    let summary_bytes = fs::read(root.join("summary.json")).map_err(display_error)?;
    let samples_bytes = fs::read(root.join("samples.jsonl")).map_err(display_error)?;
    let run = serde_json::from_slice(&run_bytes).map_err(display_error)?;
    let summary = serde_json::from_slice(&summary_bytes).map_err(display_error)?;
    let samples = String::from_utf8(samples_bytes.clone())
        .map_err(display_error)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(display_error))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EvidenceDirectory {
        run,
        summary,
        samples,
        digests: EvidenceDigest {
            label,
            run_digest: digest_bytes(&run_bytes),
            summary_digest: digest_bytes(&summary_bytes),
            samples_digest: digest_bytes(&samples_bytes),
        },
    })
}

fn gate_live_commands(candidate: &EvidenceDirectory, checks: &mut Vec<GateCheck>) {
    check(
        checks,
        "live-schema",
        schema(&candidate.summary) == Some("criv.discovery-command-summary.v1"),
        "live command summary uses the supported schema",
    );

    let live = stable_attempt(candidate, "case", "watch_live_ready");
    check(
        checks,
        "live-five-stable-samples",
        live.is_ok(),
        result_text(&live),
    );
    if let Ok(attempt) = live {
        let ready = metric(
            &candidate.summary,
            "case",
            "watch_live_ready",
            attempt,
            "publication_ready_seconds",
        );
        check(
            checks,
            "live-readiness-median",
            ready.is_some_and(|metric| metric.median <= 1.25),
            metric_detail(ready, "<= 1.25 s"),
        );
        check(
            checks,
            "live-readiness-maximum",
            ready.is_some_and(|metric| metric.maximum <= 2.0),
            metric_detail(ready, "<= 2.0 s"),
        );
        let convergence = metric(
            &candidate.summary,
            "case",
            "watch_live_ready",
            attempt,
            "convergence_seconds",
        );
        check(
            checks,
            "live-convergence-maximum",
            convergence.is_some_and(|metric| metric.maximum <= 2.0),
            metric_detail(convergence, "<= 2.0 s"),
        );
        let rows = chosen_rows(candidate, "case", "watch_live_ready", attempt);
        let converged = rows.iter().all(|row| {
            row["live_matches_one_shot"].as_bool() == Some(true)
                && row["convergence_steps"].as_array().is_some_and(|steps| {
                    ["create", "rename", "delete"]
                        .iter()
                        .all(|operation| steps.iter().any(|step| step["operation"] == *operation))
                })
        });
        check(
            checks,
            "live-create-rename-delete-and-one-shot-parity",
            converged,
            "every sample contains all mutations and matches one-shot State",
        );
    }
}

fn gate_scaling(
    pair: &ScalingPair,
    baseline: &EvidenceDirectory,
    candidate: &EvidenceDirectory,
    checks: &mut Vec<GateCheck>,
) {
    let prefix = format!(
        "scaling-{}-{}-{}",
        pair.target, pair.profile, pair.selected_files
    );
    check(
        checks,
        format!("{prefix}-schema"),
        schema(&baseline.summary) == Some("criv.discovery-summary.v1")
            && schema(&candidate.summary) == Some("criv.discovery-summary.v1"),
        "both component summaries use the supported schema",
    );
    compatible_runs(&prefix, baseline, candidate, checks);
    let before = stable_attempt(baseline, "profile", &pair.profile);
    let after = stable_attempt(candidate, "profile", &pair.profile);
    check(
        checks,
        format!("{prefix}-five-stable-samples"),
        before.is_ok() && after.is_ok(),
        format!(
            "baseline: {}; candidate: {}",
            result_text(&before),
            result_text(&after)
        ),
    );
    let (Ok(before_attempt), Ok(after_attempt)) = (before, after) else {
        return;
    };
    let before_summary = attempt_value(&baseline.summary, "profile", &pair.profile, before_attempt);
    let after_summary = attempt_value(&candidate.summary, "profile", &pair.profile, after_attempt);
    let identity_matches = before_summary
        .zip(after_summary)
        .is_some_and(|(left, right)| {
            left["selected_files"].as_u64() == Some(pair.selected_files as u64)
                && right["selected_files"].as_u64() == Some(pair.selected_files as u64)
                && left["path_digest"] == right["path_digest"]
        });
    check(
        checks,
        format!("{prefix}-path-identity"),
        identity_matches,
        "selected count and path digest match the approved scaling workload",
    );

    let elapsed_ratio_limit = scaling_ratio_limit(&pair.profile, pair.selected_files);
    gate_scaling_metric(
        &prefix,
        baseline,
        candidate,
        &pair.profile,
        (before_attempt, after_attempt),
        ("real_seconds", elapsed_ratio_limit, None),
        checks,
    );
    gate_scaling_metric(
        &prefix,
        baseline,
        candidate,
        &pair.profile,
        (before_attempt, after_attempt),
        ("peak_rss_bytes", 1.10, None),
        checks,
    );
}

fn scaling_ratio_limit(profile: &str, selected_files: usize) -> f64 {
    if matches!(profile, "source" | "source_candidates") && selected_files >= 90_000 {
        0.50
    } else {
        1.10
    }
}

fn gate_scaling_metric(
    prefix: &str,
    baseline: &EvidenceDirectory,
    candidate: &EvidenceDirectory,
    profile: &str,
    attempts: (u64, u64),
    metric_gate: (&str, f64, Option<f64>),
    checks: &mut Vec<GateCheck>,
) {
    let (before_attempt, after_attempt) = attempts;
    let (field, ratio_limit, absolute_limit) = metric_gate;
    let before = metric(&baseline.summary, "profile", profile, before_attempt, field);
    let after = metric(&candidate.summary, "profile", profile, after_attempt, field);
    let ratio = before
        .zip(after)
        .and_then(|(left, right)| (left.median > 0.0).then_some(right.median / left.median));
    check(
        checks,
        format!("{prefix}-{field}-ratio"),
        ratio.is_some_and(|value| value <= ratio_limit),
        ratio.map_or_else(
            || "metric unavailable".into(),
            |value| format!("ratio {value:.4}, limit {ratio_limit:.4}"),
        ),
    );
    if let Some(limit) = absolute_limit {
        check(
            checks,
            format!("{prefix}-{field}-absolute"),
            after.is_some_and(|value| value.median <= limit),
            metric_detail(after, &format!("<= {limit}")),
        );
    }
}

fn gate_scaling_coverage(
    input: &GateInput,
    coverage: &BTreeSet<(String, String, usize)>,
    checks: &mut Vec<GateCheck>,
) {
    for profile in ["vault", "markdown"] {
        check(
            checks,
            format!("primary-scaling-{profile}-225000"),
            coverage.contains(&(input.primary_target.clone(), profile.to_string(), 225_000)),
            "primary host has the required 250k-entry scaling pair",
        );
    }
    for target in RELEASE_TARGETS {
        for profile in ["source", "source_candidates"] {
            check(
                checks,
                format!("cross-platform-{profile}-90000-{target}"),
                coverage.contains(&(target.to_string(), profile.into(), 90_000)),
                "release platform has the matched 100k-entry Source workload",
            );
        }
    }
}

fn gate_artifacts(
    base: &Path,
    input: &GateInput,
    checks: &mut Vec<GateCheck>,
) -> Result<Vec<ArtifactReceipt>, String> {
    check(
        checks,
        "normal-dependency-count",
        input.artifacts.normal_dependencies_after <= input.artifacts.normal_dependencies_before,
        format!(
            "before {}, after {}",
            input.artifacts.normal_dependencies_before, input.artifacts.normal_dependencies_after
        ),
    );
    check(
        checks,
        "no-new-native-toolchain",
        !input.artifacts.native_compiler_or_library_added,
        "no new native compiler, bindgen, libclang, or native library",
    );
    let targets = input
        .artifacts
        .targets
        .iter()
        .map(|item| item.target.as_str())
        .collect::<BTreeSet<_>>();
    check(
        checks,
        "four-release-artifacts",
        targets == RELEASE_TARGETS.into_iter().collect(),
        format!(
            "targets: {}",
            targets.into_iter().collect::<Vec<_>>().join(", ")
        ),
    );

    let mut receipts = Vec::new();
    for target in &input.artifacts.targets {
        let prefix = format!("artifact-{}", target.target);
        if target.candidate_binary.is_absolute()
            || target
                .candidate_binary
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(format!(
                "candidate binary path must be a safe relative path: {}",
                target.candidate_binary.display()
            ));
        }
        let binary = base.join(&target.candidate_binary);
        let bytes = fs::read(&binary).map_err(|error| {
            format!(
                "failed to read candidate binary {}: {error}",
                binary.display()
            )
        })?;
        let digest = digest_bytes(&bytes);
        check(
            checks,
            format!("{prefix}-commit"),
            target.commit == input.commit,
            format!("artifact commit {}", target.commit),
        );
        check(
            checks,
            format!("{prefix}-binary-size"),
            bytes.len() as u64 <= target.baseline_binary_bytes,
            format!(
                "candidate {}, baseline {}",
                bytes.len(),
                target.baseline_binary_bytes
            ),
        );
        let builds_valid = target.baseline_build_seconds.len() == 3
            && target.candidate_build_seconds.len() == 3
            && target
                .baseline_build_seconds
                .iter()
                .all(|value| *value > 0.0)
            && target
                .candidate_build_seconds
                .iter()
                .all(|value| *value > 0.0);
        let build_ratio = builds_valid.then(|| {
            median(target.candidate_build_seconds.clone())
                / median(target.baseline_build_seconds.clone())
        });
        check(
            checks,
            format!("{prefix}-clean-builds"),
            builds_valid
                && target.clean_builds
                && target.compiler_cache_disabled
                && target.registry_inputs_present,
            "three clean builds, no compiler cache, and registry inputs present",
        );
        check(
            checks,
            format!("{prefix}-build-time"),
            build_ratio.is_some_and(|value| value <= 1.10),
            build_ratio.map_or_else(
                || "build samples invalid".into(),
                |value| format!("ratio {value:.4}, limit 1.1000"),
            ),
        );
        check(
            checks,
            format!("{prefix}-baseline-digest"),
            target.baseline_binary_digest.len() == 64
                && target
                    .baseline_binary_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            format!("baseline digest {}", target.baseline_binary_digest),
        );
        receipts.push(ArtifactReceipt {
            target: target.target.clone(),
            path: target.candidate_binary.display().to_string(),
            digest,
            sha256: sha256_file(&binary)?,
            file_name: binary
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    format!(
                        "candidate binary has no UTF-8 file name: {}",
                        binary.display()
                    )
                })?
                .to_string(),
            bytes: bytes.len() as u64,
        });
    }
    receipts.sort_by(|left, right| left.target.cmp(&right.target));
    Ok(receipts)
}

fn compatible_runs(
    prefix: &str,
    baseline: &EvidenceDirectory,
    candidate: &EvidenceDirectory,
    checks: &mut Vec<GateCheck>,
) {
    for field in ["workload_digest", "machine_digest", "samples"] {
        check(
            checks,
            format!("{prefix}-matched-{field}"),
            baseline.run[field] == candidate.run[field],
            format!(
                "baseline {}, candidate {}",
                baseline.run[field], candidate.run[field]
            ),
        );
    }
}

fn stable_attempt(evidence: &EvidenceDirectory, key: &str, value: &str) -> Result<u64, String> {
    let attempts = evidence.summary["attempts"]
        .as_array()
        .ok_or_else(|| "summary has no attempts".to_string())?;
    let mut matching = attempts
        .iter()
        .filter(|attempt| attempt[key].as_str() == Some(value))
        .collect::<Vec<_>>();
    matching.sort_by_key(|attempt| attempt["attempt"].as_u64().unwrap_or(0));
    let first = matching
        .first()
        .ok_or_else(|| format!("no attempt for {key}={value}"))?;
    let selected = if first["unstable"].as_bool() == Some(true) {
        matching
            .get(1)
            .ok_or_else(|| "unstable first attempt has no complete repeat".to_string())?
    } else {
        first
    };
    if selected["unstable"].as_bool() != Some(false)
        || selected["successful_samples"].as_u64() != Some(5)
        || selected["failed_samples"].as_u64() != Some(0)
    {
        return Err("selected attempt is not five stable successful samples".into());
    }
    selected["attempt"]
        .as_u64()
        .ok_or_else(|| "attempt number is missing".into())
}

#[derive(Clone, Copy)]
struct Metric {
    median: f64,
    maximum: f64,
}

fn metric(summary: &Value, key: &str, value: &str, attempt: u64, field: &str) -> Option<Metric> {
    let value = attempt_value(summary, key, value, attempt)?.get(field)?;
    Some(Metric {
        median: value["median"].as_f64()?,
        maximum: value["maximum"].as_f64()?,
    })
}

fn attempt_value<'a>(
    summary: &'a Value,
    key: &str,
    value: &str,
    attempt: u64,
) -> Option<&'a Value> {
    summary["attempts"]
        .as_array()?
        .iter()
        .find(|item| item[key].as_str() == Some(value) && item["attempt"].as_u64() == Some(attempt))
}

fn chosen_rows<'a>(
    evidence: &'a EvidenceDirectory,
    key: &str,
    value: &str,
    attempt: u64,
) -> Vec<&'a Value> {
    let mut rows = evidence
        .samples
        .iter()
        .filter(|row| {
            row[key].as_str() == Some(value)
                && row["attempt"].as_u64() == Some(attempt)
                && (row["successful"].as_bool() == Some(true)
                    || row["exit_status"].as_i64() == Some(0))
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row["sample"].as_u64().unwrap_or(0));
    rows
}

fn schema(value: &Value) -> Option<&str> {
    value["schema"].as_str()
}

fn check(
    checks: &mut Vec<GateCheck>,
    id: impl Into<String>,
    passed: bool,
    detail: impl Into<String>,
) {
    checks.push(GateCheck {
        id: id.into(),
        passed,
        detail: detail.into(),
    });
}

fn result_text<T>(result: &Result<T, String>) -> String {
    match result {
        Ok(_) => "valid".into(),
        Err(error) => error.clone(),
    }
}

fn metric_detail(metric: Option<Metric>, limit: &str) -> String {
    metric.map_or_else(
        || "metric unavailable".into(),
        |value| {
            format!(
                "median {:.6}, maximum {:.6}, limit {limit}",
                value.median, value.maximum
            )
        },
    )
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(display_error)?;
    }
    let mut bytes = serde_json::to_vec_pretty(value).map_err(display_error)?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(display_error)
}

fn digest_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn sha256_file(path: &Path) -> Result<String, String> {
    for (program, args) in [
        ("sha256sum", vec![path.as_os_str()]),
        (
            "shasum",
            vec![
                std::ffi::OsStr::new("-a"),
                std::ffi::OsStr::new("256"),
                path.as_os_str(),
            ],
        ),
    ] {
        let output = match Command::new(program).args(args).output() {
            Ok(output) => output,
            Err(_) => continue,
        };
        if output.status.success() {
            let text = String::from_utf8(output.stdout).map_err(display_error)?;
            if let Some(digest) = text.split_whitespace().next()
                && digest.len() == 64
                && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Ok(digest.to_ascii_lowercase());
            }
        }
    }
    Err(format!(
        "no working SHA-256 command is available for {}",
        path.display()
    ))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_scaling_ratio_limits_are_locked() {
        assert_eq!(scaling_ratio_limit("source", 90_000), 0.50);
        assert_eq!(scaling_ratio_limit("source_candidates", 90_000), 0.50);
        assert_eq!(scaling_ratio_limit("vault", 225_000), 1.10);
        assert_eq!(scaling_ratio_limit("markdown", 225_000), 1.10);
    }

    #[test]
    fn hosted_scaling_coverage_uses_four_100k_source_hosts_and_250k_document_profiles() {
        let input = GateInput {
            schema: INPUT_SCHEMA.into(),
            commit: "a".repeat(40),
            toolchain: "1.97.1".into(),
            valid_until_unix: u64::MAX,
            primary_target: "aarch64-apple-darwin".into(),
            live_commands: "live".into(),
            scaling: vec![],
            artifacts: ArtifactEvidence {
                normal_dependencies_before: 1,
                normal_dependencies_after: 1,
                native_compiler_or_library_added: false,
                targets: vec![],
            },
        };
        let mut coverage = BTreeSet::new();
        for target in RELEASE_TARGETS {
            for profile in ["source", "source_candidates"] {
                coverage.insert((target.into(), profile.into(), 90_000));
            }
        }
        for profile in ["vault", "markdown"] {
            coverage.insert((input.primary_target.clone(), profile.into(), 225_000));
        }
        let mut checks = Vec::new();
        gate_scaling_coverage(&input, &coverage, &mut checks);
        assert_eq!(checks.len(), 10);
        assert!(checks.iter().all(|check| check.passed));
    }

    #[test]
    fn median_uses_the_middle_of_sorted_samples() {
        assert_eq!(median(vec![3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(vec![4.0, 1.0, 3.0, 2.0]), 2.5);
    }
}
