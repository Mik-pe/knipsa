#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
binary=$($repo_root/benchmarks/reference/build-clipper2.sh)
python3 "$repo_root/benchmarks/reference/clipper2/encode-workload.py" \
  "${1:-$repo_root/benchmarks/integer-workloads.json}" | \
  KNIPSA_COORDINATE_TYPE=i64 "$binary"
