#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
boost_version=1.91.0
boost_archive_name=boost_1_91_0.tar.bz2
boost_sha256=de5e6b0e4913395c6bdfa90537febd9028ea4c0735d2cdb0cd9b45d5f51264f5
reference_root=$repo_root/target/reference
boost_root=$reference_root/boost_1_91_0
boost_archive=$reference_root/$boost_archive_name
build_root=$reference_root/boost-build
binary=$build_root/knipsa-boost-benchmark

mkdir -p "$reference_root"
actual_sha256=
if [ -f "$boost_archive" ]; then
  actual_sha256=$(shasum -a 256 "$boost_archive" | awk '{print $1}')
fi
if [ "$actual_sha256" != "$boost_sha256" ]; then
  temporary_archive=$boost_archive.download.$$
  trap 'rm -f "$temporary_archive"' EXIT HUP INT TERM
  curl --fail --location --silent --show-error \
    "https://archives.boost.io/release/$boost_version/source/$boost_archive_name" \
    --output "$temporary_archive"
  downloaded_sha256=$(shasum -a 256 "$temporary_archive" | awk '{print $1}')
  if [ "$downloaded_sha256" != "$boost_sha256" ]; then
    echo "Boost checksum mismatch: expected $boost_sha256, got $downloaded_sha256" >&2
    exit 1
  fi
  mv "$temporary_archive" "$boost_archive"
  trap - EXIT HUP INT TERM
fi

if [ ! -f "$boost_root/boost/geometry.hpp" ]; then
  if [ -e "$boost_root" ]; then
    echo "incomplete Boost extraction at $boost_root" >&2
    exit 1
  fi
  extraction_root=$(mktemp -d "$reference_root/boost-extract.XXXXXX")
  trap 'rm -rf "$extraction_root"' EXIT HUP INT TERM
  tar -xjf "$boost_archive" -C "$extraction_root"
  mv "$extraction_root/boost_1_91_0" "$boost_root"
  rmdir "$extraction_root"
  trap - EXIT HUP INT TERM
fi

mkdir -p "$build_root"
cxx=${CXX:-clang++}
"$cxx" -O3 -DNDEBUG -std=c++17 -Wall -Wextra -Wpedantic -Werror \
  -I"$repo_root/benchmarks/reference/cpp" -isystem "$boost_root" \
  "$repo_root/benchmarks/reference/boost/adapter.cpp" -o "$binary"

printf '%s\n' "$binary"
