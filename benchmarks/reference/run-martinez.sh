#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
workdir="$repo_root/target/reference/martinez"
mkdir -p "$workdir"
npm install --prefix "$workdir" --ignore-scripts --no-package-lock \
  martinez-polygon-clipping@0.8.1 >/dev/null
cp "$repo_root/benchmarks/reference/martinez/adapter.mjs" "$workdir/adapter.mjs"
node "$workdir/adapter.mjs" "${1:-$repo_root/benchmarks/workloads.json}"
