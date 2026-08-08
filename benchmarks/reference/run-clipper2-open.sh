#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
clipper_root=${KNIPSA_CLIPPER2_ROOT:-$repo_root/target/reference/clipper2}
build_root=$repo_root/target/reference/clipper2-build
binary=$build_root/knipsa-clipper2-open-benchmark
clipper_cpp=$clipper_root/CPP
mkdir -p "$build_root"
${CXX:-clang++} -O3 -DNDEBUG -std=c++17 -Wall -Wextra -Wpedantic \
  -I"$clipper_cpp/Clipper2Lib/include" \
  "$repo_root/benchmarks/reference/clipper2/open-adapter.cpp" \
  "$clipper_cpp/Clipper2Lib/src/clipper.engine.cpp" \
  "$clipper_cpp/Clipper2Lib/src/clipper.offset.cpp" \
  "$clipper_cpp/Clipper2Lib/src/clipper.rectclip.cpp" \
  "$clipper_cpp/Clipper2Lib/src/clipper.triangulation.cpp" -o "$binary"
python3 "$repo_root/benchmarks/reference/clipper2/encode-open-workload.py" \
  "${1:-$repo_root/benchmarks/open-workloads.json}" | "$binary"
