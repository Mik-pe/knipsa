#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cargo_fuzz=${CARGO_FUZZ:-cargo-fuzz}

if ! command -v "$cargo_fuzz" >/dev/null 2>&1; then
  if [ -x "$repo_root/target/tools/bin/cargo-fuzz" ]; then
    cargo_fuzz=$repo_root/target/tools/bin/cargo-fuzz
  else
    echo "cargo-fuzz is required; install it with: cargo install cargo-fuzz" >&2
    exit 2
  fi
fi

for target in geometry_inputs boolean_inputs double_boolean_inputs; do
  RUSTUP_TOOLCHAIN=nightly "$cargo_fuzz" run "$target" \
    "$repo_root/fuzz/corpus/$target" -- \
    -seed=1263421779 -runs=1 -max_len=256 -timeout=10
done
