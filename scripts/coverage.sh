#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "cargo-llvm-cov is required; install it with: cargo install cargo-llvm-cov" >&2
  exit 2
fi

mkdir -p target/coverage
cargo +nightly llvm-cov --workspace --all-features \
  --branch \
  --lcov --output-path target/coverage/lcov.info \
  --summary-only \
  --fail-under-lines 100 \
  --fail-under-functions 100

branch_found=$(awk -F: '/^BRF:/{ total += $2 } END { print total + 0 }' target/coverage/lcov.info)
branch_hit=$(awk -F: '/^BRH:/{ total += $2 } END { print total + 0 }' target/coverage/lcov.info)
if [ "$branch_found" -ne "$branch_hit" ]; then
  echo "branch coverage is ${branch_hit}/${branch_found}, expected 100%" >&2
  exit 1
fi
