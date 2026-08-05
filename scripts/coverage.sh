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
  --fail-under-lines 100 \
  --fail-under-functions 100

branch_found=$(awk -F: '/^BRF:/{ total += $2 } END { print total + 0 }' target/coverage/lcov.info)
branch_records=$(awk '/^BRDA:/{ total += 1 } END { print total + 0 }' target/coverage/lcov.info)
branch_missed=$(awk -F, '/^BRDA:/ && ($4 == "-" || $4 == "" || ($4 + 0) == 0) { total += 1 } END { print total + 0 }' target/coverage/lcov.info)

# BRH is an aggregate summary that can undercount Rust generic instantiations.
# Gate the individual BRDA records instead: every true/false branch must have
# a non-zero execution count, and every branch declared by BRF must be present.
if [ "$branch_records" -ne "$branch_found" ]; then
  echo "branch detail records are ${branch_records}/${branch_found}; expected one BRDA record per branch" >&2
  exit 1
fi
if [ "$branch_missed" -ne 0 ]; then
  echo "branch coverage has ${branch_missed} missed detailed branch records" >&2
  exit 1
fi
