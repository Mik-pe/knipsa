#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
pythonpath="$repo_root/target/reference/shapely"
PYTHONPATH="$pythonpath${PYTHONPATH:+:$PYTHONPATH}" \
  python3 "$repo_root/benchmarks/reference/geos/adapter.py" \
  "${1:-$repo_root/benchmarks/workloads.json}"
