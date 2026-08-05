#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
clipper_revision=f9c5eb6e14a59f6f5d65fbfb3564519a561cf4fd
clipper_root=${KNIPSA_CLIPPER2_ROOT:-$repo_root/target/reference/clipper2}
build_root=$repo_root/target/reference/clipper2-build
binary=$build_root/knipsa-clipper2-benchmark

if [ ! -d "$clipper_root/.git" ]; then
  mkdir -p "$(dirname "$clipper_root")"
  git clone --quiet https://github.com/AngusJohnson/Clipper2.git "$clipper_root"
fi
git -C "$clipper_root" fetch --quiet --depth=1 origin "$clipper_revision" 2>/dev/null || true
git -C "$clipper_root" checkout --quiet "$clipper_revision"

mkdir -p "$build_root"
clipper_cpp="$clipper_root/CPP"
cxx=${CXX:-clang++}

"$cxx" -O3 -DNDEBUG -std=c++17 -Wall -Wextra -Wpedantic \
  -I"$clipper_cpp/Clipper2Lib/include" \
  "$repo_root/benchmarks/reference/clipper2/adapter.cpp" \
  "$clipper_cpp/Clipper2Lib/src/clipper.engine.cpp" \
  "$clipper_cpp/Clipper2Lib/src/clipper.offset.cpp" \
  "$clipper_cpp/Clipper2Lib/src/clipper.rectclip.cpp" \
  "$clipper_cpp/Clipper2Lib/src/clipper.triangulation.cpp" \
  -o "$binary"

printf '%s\n' "$binary"
