#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 8 ]]; then
  echo "usage: render-git-note.sh RESULT_DIR OUTPUT COMMIT REF WORKFLOW_URL WORKFLOW_RUN_ID WORKFLOW_ATTEMPT ARTIFACT_NAME" >&2
  exit 2
fi

result_dir="$1"
output="$2"
commit="$3"
pushed_ref="$4"
workflow_url="$5"
workflow_run_id="$6"
workflow_attempt="$7"
artifact_name="$8"

for required in run.json summary.json samples.jsonl; do
  if [[ ! -f "$result_dir/$required" ]]; then
    echo "missing performance evidence: $result_dir/$required" >&2
    exit 1
  fi
done

jq --null-input --sort-keys \
  --slurpfile run "$result_dir/run.json" \
  --slurpfile summary "$result_dir/summary.json" \
  --slurpfile samples "$result_dir/samples.jsonl" \
  --arg commit "$commit" \
  --arg pushed_ref "$pushed_ref" \
  --arg workflow_url "$workflow_url" \
  --arg workflow_run_id "$workflow_run_id" \
  --arg workflow_attempt "$workflow_attempt" \
  --arg artifact_name "$artifact_name" \
  '
    if $run[0].schema != "criv.performance-run.v2" then
      error("unsupported performance run schema")
    elif $summary[0].schema != "criv.performance-summary.v2" then
      error("unsupported performance summary schema")
    elif any($samples[]; .schema != "criv.performance-sample.v2") then
      error("unsupported performance sample schema")
    elif $run[0].revision != $commit then
      error("performance revision \($run[0].revision) does not match note commit \($commit)")
    elif $run[0].dirty != false then
      error("performance run must use a clean checkout")
    elif $run[0].profile != "release" then
      error("performance Git notes require a release profile")
    elif $run[0].samples < 3 then
      error("performance Git notes require at least three samples")
    elif any($samples[]; .run_id != $run[0].run_id or .exit_status != 0) then
      error("raw samples must belong to the run and succeed")
    elif any($summary[0].cases[]; .failed_samples != 0 or .successful_samples != $run[0].samples) then
      error("timing summaries do not contain the declared successful sample count")
    else
      {
        schema: "criv.performance-git-note.v2",
        commit: $commit,
        pushed_ref: $pushed_ref,
        workflow: {
          url: $workflow_url,
          run_id: $workflow_run_id,
          attempt: $workflow_attempt
        },
        artifact: $artifact_name,
        report: {
          format: "html",
          artifact_path: "report.html"
        },
        evidence: {
          run_id: $run[0].run_id,
          profile: $run[0].profile,
          observation: "external-subprocess",
          samples: $run[0].samples,
          binary_digest: $run[0].binary_digest,
          machine_digest: $run[0].machine.digest,
          workloads: [
            $run[0].manifests[]
            | {id, tier, digest}
          ]
        },
        timings: [
          $summary[0].cases[]
          | {
              workload,
              workload_digest,
              case,
              cache_state,
              successful_samples,
              failed_samples,
              real_seconds,
              user_seconds,
              system_seconds
            }
        ]
      }
    end
  ' >"$output"
