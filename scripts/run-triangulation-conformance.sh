#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
workload=${1:-$repo_root/benchmarks/triangulation-workloads.json}
output_dir=${2:-$repo_root/target/conformance-triangulation}

case "$workload" in
  /*) ;;
  *) workload="$repo_root/$workload" ;;
esac
case "$output_dir" in
  /*) ;;
  *) output_dir="$repo_root/$output_dir" ;;
esac

mkdir -p "$output_dir"
KNIPSA_TRIANGULATION_WORKLOAD="$workload" \
  cargo bench -p knipsa --bench triangulation_workload -- --nocapture \
  >"$output_dir/knipsa.jsonl"
"$repo_root/benchmarks/reference/run-clipper2-triangulation.sh" "$workload" \
  >"$output_dir/clipper2.jsonl"
python3 "$repo_root/scripts/compare-triangulation-results.py" \
  "$workload" "$output_dir/knipsa.jsonl" "$output_dir/clipper2.jsonl"
