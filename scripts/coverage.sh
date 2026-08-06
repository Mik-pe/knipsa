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
  --lcov --output-path target/coverage/lcov.info

# A workspace run builds the library both as its unit-test target and as a
# dependency of knipsa-ffi. LLVM therefore emits duplicate crate and generic
# symbols for one Rust function definition. Gate source definitions rather
# than raw codegen instances: every definition line must have a non-zero hit
# in at least one emitted instance.
set -- $(awk '
  /^SF:/ { file = substr($0, 4); next }
  /^FN:/ {
    record = substr($0, 4)
    comma = index(record, ",")
    line = substr(record, 1, comma - 1)
    name = substr(record, comma + 1)
    symbol_definition[file SUBSEP name] = file SUBSEP line
    definitions[file SUBSEP line] = 1
    next
  }
  /^FNDA:/ {
    record = substr($0, 6)
    comma = index(record, ",")
    hits = substr(record, 1, comma - 1) + 0
    name = substr(record, comma + 1)
    key = symbol_definition[file SUBSEP name]
    if (key != "") {
      definition_hits[key] += hits
    }
  }
  END {
    for (key in definitions) {
      total += 1
      if ((definition_hits[key] + 0) == 0) {
        missed += 1
        split(key, fields, SUBSEP)
        printf "uncovered function definition: %s:%s\n", fields[1], fields[2] > "/dev/stderr"
      }
    }
    print total + 0, missed + 0
  }
' target/coverage/lcov.info)
function_records=$1
function_missed=$2

line_records=$(awk '/^DA:/{ total += 1 } END { print total + 0 }' target/coverage/lcov.info)
line_missed=$(awk -F, '/^DA:/ && ($2 == "" || ($2 + 0) == 0) { total += 1 } END { print total + 0 }' target/coverage/lcov.info)

branch_found=$(awk -F: '/^BRF:/{ total += $2 } END { print total + 0 }' target/coverage/lcov.info)
branch_records=$(awk '/^BRDA:/{ total += 1 } END { print total + 0 }' target/coverage/lcov.info)
branch_missed=$(awk -F, '/^BRDA:/ && ($4 == "-" || $4 == "" || ($4 + 0) == 0) { total += 1 } END { print total + 0 }' target/coverage/lcov.info)

# LLVM's aggregate function totals count duplicate crate and generic codegen
# instances. The source-definition gate above requires every Rust function
# definition to execute while remaining stable across compiler codegen changes.
if [ "$function_records" -eq 0 ] || [ "$function_missed" -ne 0 ]; then
  echo "function coverage has ${function_missed} missed source definitions out of ${function_records}" >&2
  exit 1
fi
# LLVM's aggregate line totals can count Rust inline/generic instantiations for
# which it emits no corresponding source record. Gate every detailed source
# record instead, just as branch coverage below gates every detailed branch.
if [ "$line_records" -eq 0 ] || [ "$line_missed" -ne 0 ]; then
  echo "line coverage has ${line_missed} missed detailed records out of ${line_records}" >&2
  exit 1
fi
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
