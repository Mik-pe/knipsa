#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

cargo build -p knipsa-ffi
mkdir -p target/c-api

cc=${CC:-clang}
"$cc" -std=c11 -Wall -Wextra -Werror \
  -Icrates/knipsa-ffi/include \
  tests/c/abi_smoke.c \
  -Ltarget/debug -lknipsa_ffi \
  -Wl,-rpath,"$repo_root/target/debug" \
  -o target/c-api/abi-smoke

target/c-api/abi-smoke

case "$(uname -s)" in
  Darwin)
    nm -gU target/debug/libknipsa_ffi.dylib
    ;;
  *)
    nm -g target/debug/libknipsa_ffi.so
    ;;
esac | grep -E 'knipsa_(version|status_message|validate_paths64|point_in_polygon64)$' >/dev/null

echo "C API smoke test passed"
