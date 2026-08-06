#!/usr/bin/env bash
set -euo pipefail

root=${1:?workspace root is required}
if [[ ${GITHUB_ACTIONS:-} != true || ${GITHUB_HEAD_REF:-} != agent/clipper2-validation-run ]]; then
  echo "Skipping temporary Clipper2 validation outside its dedicated CI branch"
  exit 0
fi

scratch=${RUNNER_TEMP:-/tmp}/knipsa-clipper2-validation-${GITHUB_RUN_ID:-$$}
candidate=$scratch/candidate
baseline=$scratch/baseline
results=$scratch/results
mkdir -p "$scratch" "$results"

cleanup() {
  git -C "$root" worktree remove --force "$candidate" >/dev/null 2>&1 || true
  git -C "$root" worktree remove --force "$baseline" >/dev/null 2>&1 || true
}
trap cleanup EXIT

git -C "$root" fetch --no-tags --depth=1 origin main
git -C "$root" worktree add --detach "$candidate" HEAD
git -C "$root" worktree add --detach "$baseline" origin/main

(
  cd "$candidate"
  git apply --check .github/agent-clipper2.patch
  git apply .github/agent-clipper2.patch
  rm .github/agent-clipper2.patch
  cargo +stable fmt --all
  git diff --check
)

build_benchmark() {
  local source=$1
  local target=$2
  (
    cd "$source"
    CARGO_TARGET_DIR="$target" cargo +stable build --release -p knipsa --bench workload
  )
  find "$target/release/deps" -maxdepth 1 -type f -name 'workload-*' ! -name '*.d' -executable \
    | head -n 1
}

baseline_binary=$(build_benchmark "$baseline" "$scratch/target-baseline")
candidate_binary=$(build_benchmark "$candidate" "$scratch/target-candidate")
test -n "$baseline_binary"
test -n "$candidate_binary"

{
  printf 'base_sha=%s\n' "$(git -C "$baseline" rev-parse HEAD)"
  printf 'candidate_sha=%s\n' "$(git -C "$root" rev-parse HEAD)"
  printf 'date_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  uname -a
  lscpu || true
  cargo +stable --version
  rustc +stable --version --verbose
} > "$results/metadata.txt"

for round in 1 2 3 4 5 6 7; do
  if (( round % 2 == 1 )); then
    KNIPSA_WORKLOAD="$candidate/benchmarks/workloads.json" \
      "$baseline_binary" > "$results/baseline-$round.jsonl"
    KNIPSA_WORKLOAD="$candidate/benchmarks/workloads.json" \
      "$candidate_binary" > "$results/candidate-$round.jsonl"
  else
    KNIPSA_WORKLOAD="$candidate/benchmarks/workloads.json" \
      "$candidate_binary" > "$results/candidate-$round.jsonl"
    KNIPSA_WORKLOAD="$candidate/benchmarks/workloads.json" \
      "$baseline_binary" > "$results/baseline-$round.jsonl"
  fi
  (
    cd "$candidate"
    benchmarks/reference/run-clipper2.sh benchmarks/workloads.json
  ) > "$results/clipper2-$round.jsonl"

  for implementation in baseline candidate clipper2; do
    jq -r 'select(has("id")) | [.id, .status, .signature] | @tsv' \
      "$results/$implementation-$round.jsonl" \
      > "$results/$implementation-$round.signature.tsv"
  done
  diff -u "$results/baseline-$round.signature.tsv" \
    "$results/candidate-$round.signature.tsv"
  diff -u "$results/baseline-$round.signature.tsv" \
    "$results/clipper2-$round.signature.tsv"
done

jq -r 'select(has("id")) | .id' "$results/candidate-1.jsonl" > "$results/case-ids.txt"
printf 'case\tbaseline_ns\tcandidate_ns\tspeedup\tclipper2_ns\tcandidate_vs_clipper2\n' \
  > "$results/summary.tsv"

while IFS= read -r id; do
  for implementation in baseline candidate clipper2; do
    : > "$results/$implementation.values"
    for round in 1 2 3 4 5 6 7; do
      jq -r --arg id "$id" 'select(.id == $id) | .median_ns' \
        "$results/$implementation-$round.jsonl" \
        >> "$results/$implementation.values"
    done
    sort -n "$results/$implementation.values" | sed -n '4p' \
      > "$results/$implementation.median"
  done

  baseline_ns=$(cat "$results/baseline.median")
  candidate_ns=$(cat "$results/candidate.median")
  clipper_ns=$(cat "$results/clipper2.median")
  speedup=$(awk -v b="$baseline_ns" -v c="$candidate_ns" 'BEGIN { printf "%.6f", b / c }')
  versus_clipper=$(awk -v c="$candidate_ns" -v r="$clipper_ns" 'BEGIN { printf "%.6f", r / c }')
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$id" "$baseline_ns" "$candidate_ns" "$speedup" "$clipper_ns" "$versus_clipper" \
    >> "$results/summary.tsv"
done < "$results/case-ids.txt"

gate_status=0
awk -F '\t' '
  NR > 1 {
    baseline_total += $2
    candidate_total += $3
    clipper_total += $5
    baseline_log += log($2 / $3)
    clipper_log += log($5 / $3)
    count += 1
    if ($3 < $5 * 0.98) candidate_wins += 1
    else if ($5 < $3 * 0.98) clipper_wins += 1
    else ties += 1
    if ($3 > $2 * 1.10) regressions = regressions $1 " "
    if ($1 == "high-vertex-intersection" && $2 / $3 < 2.0) high_vertex_failed = 1
    if ($1 == "high-vertex-xor" && $2 / $3 < 2.0) high_vertex_failed = 1
  }
  END {
    printf "baseline_total_ns=%.0f\n", baseline_total
    printf "candidate_total_ns=%.0f\n", candidate_total
    printf "clipper2_total_ns=%.0f\n", clipper_total
    printf "baseline_aggregate_speedup=%.6f\n", baseline_total / candidate_total
    printf "candidate_vs_clipper2_aggregate=%.6f\n", clipper_total / candidate_total
    printf "baseline_geomean_speedup=%.6f\n", exp(baseline_log / count)
    printf "candidate_vs_clipper2_geomean=%.6f\n", exp(clipper_log / count)
    printf "candidate_wins=%d\nclipper2_wins=%d\nties=%d\n", candidate_wins, clipper_wins, ties
    printf "regressions_over_10pct=%s\n", regressions
    if (candidate_total * 1.02 >= clipper_total || regressions != "" || high_vertex_failed) exit 42
  }
' "$results/summary.tsv" > "$results/aggregate.txt" || gate_status=$?

printf '\n=== Native Clipper2 validation matrix ===\n'
cat "$results/summary.tsv"
printf '\n=== Aggregate ===\n'
cat "$results/aggregate.txt"
printf '\nRaw evidence: %s\n' "$results"

if [[ $gate_status -ne 0 ]]; then
  echo "Candidate failed the aggregate, regression, or high-vertex performance gate"
  exit "$gate_status"
fi
