#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
workload=${1:-$repo_root/benchmarks/workloads.json}
output_dir=${2:-$repo_root/target/conformance}

mkdir -p "$output_dir"

cargo bench -p knipsa --bench workload -- --nocapture >"$output_dir/knipsa.jsonl"
"$repo_root/benchmarks/reference/run-clipper2.sh" "$workload" >"$output_dir/clipper2.jsonl"
"$repo_root/benchmarks/reference/run-geos.sh" "$workload" >"$output_dir/geos.jsonl"
"$repo_root/benchmarks/reference/run-martinez.sh" "$workload" >"$output_dir/martinez.jsonl"

for reference in clipper2 geos martinez; do
  python3 "$repo_root/scripts/compare-benchmark-results.py" \
    --workload "$workload" \
    "$output_dir/knipsa.jsonl" \
    "$output_dir/$reference.jsonl"
done
