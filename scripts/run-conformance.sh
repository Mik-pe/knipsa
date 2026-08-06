#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
workload=${1:-$repo_root/benchmarks/workloads.json}
output_dir=${2:-$repo_root/target/conformance}
references=${3:-"clipper2 geos martinez"}

case "$workload" in
  /*) ;;
  *) workload="$repo_root/$workload" ;;
esac
case "$output_dir" in
  /*) ;;
  *) output_dir="$repo_root/$output_dir" ;;
esac

mkdir -p "$output_dir"

KNIPSA_WORKLOAD="$workload" cargo bench -p knipsa --bench workload -- --nocapture \
  >"$output_dir/knipsa.jsonl"

for reference in $references; do
  case "$reference" in
    clipper2|geos|martinez) ;;
    *) echo "unknown reference adapter: $reference" >&2; exit 2 ;;
  esac
  "$repo_root/benchmarks/reference/run-$reference.sh" "$workload" \
    >"$output_dir/$reference.jsonl"
  python3 "$repo_root/scripts/compare-benchmark-results.py" \
    --workload "$workload" \
    "$output_dir/knipsa.jsonl" \
    "$output_dir/$reference.jsonl"
done
