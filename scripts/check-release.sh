#!/bin/sh
set -eu

repo_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

package_id=$(cargo pkgid -p knipsa)
version=${package_id##*#}
core_archive="target/package/knipsa-$version.crate"
core_directory="target/package/knipsa-$version"

test "$version" = "0.2.1" || {
  echo "release fixture expects 0.2.1, workspace is $version" >&2
  exit 1
}

cargo package --locked --allow-dirty -p knipsa
test -f "$core_archive"
test -d "$core_directory"

cargo publish --dry-run --locked --allow-dirty -p knipsa
cargo package --allow-dirty -p knipsa-ffi --list | grep -F 'include/knipsa.h' >/dev/null
CARGO_TARGET_DIR=target/release-feature-check \
  cargo check --quiet --locked --manifest-path "$core_directory/Cargo.toml" --no-default-features
for features in geo-types serde geo-types,serde; do
  CARGO_TARGET_DIR=target/release-feature-check \
    cargo check --quiet --locked --manifest-path "$core_directory/Cargo.toml" \
    --no-default-features --features "$features"
done
CARGO_TARGET_DIR=target/release-consumer \
  cargo run --quiet --locked --manifest-path release-tests/rust-consumer/Cargo.toml
cargo test --locked -p knipsa-ffi

core_size=$(wc -c < "$core_archive" | tr -d ' ')
echo "release artifact verified: knipsa-$version ($core_size bytes)"
echo "knipsa-ffi package and publish dry-run are intentionally deferred until knipsa-$version is visible on crates.io"
