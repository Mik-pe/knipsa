#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
binary=$($repo_root/benchmarks/reference/build-boost.sh)
python3 "$repo_root/benchmarks/reference/clipper2/encode-workload.py" \
  "${1:-$repo_root/benchmarks/workloads.json}" | "$binary"
