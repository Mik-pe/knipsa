#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
workload=${1:-$repo_root/benchmarks/offset-workloads.json}
output_dir=${2:-$repo_root/target/conformance-offset}

case "$workload" in
  /*) ;;
  *) workload="$repo_root/$workload" ;;
esac
case "$output_dir" in
  /*) ;;
  *) output_dir="$repo_root/$output_dir" ;;
esac

mkdir -p "$output_dir"
KNIPSA_OFFSET_WORKLOAD="$workload" \
  cargo bench -p knipsa --bench offset_workload -- --nocapture \
  >"$output_dir/knipsa.jsonl"
"$repo_root/benchmarks/reference/run-clipper2-offset.sh" "$workload" \
  >"$output_dir/clipper2.jsonl"
python3 "$repo_root/scripts/compare-offset-results.py" \
  "$workload" "$output_dir/knipsa.jsonl" "$output_dir/clipper2.jsonl"
