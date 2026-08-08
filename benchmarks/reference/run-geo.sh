#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
manifest="$repo_root/benchmarks/reference/geo/Cargo.toml"
workload=${1:-$repo_root/benchmarks/workloads.json}

CARGO_TARGET_DIR="$repo_root/target/reference/geo-target" \
  cargo +stable run --quiet --release --locked --manifest-path "$manifest" -- "$workload"
