# knipsa

![A crab claw clipping a triangle](assets/knipsa-crab-clips-triangle.png)

`knipsa` is a polygon geometry library written in Rust. It is designed to be
safe, predictable, and easy to call from other languages through its small
C-compatible interface.

## Status

Early development. The current code provides checked integer geometry,
path validation and normalization, point-in-polygon classification, request
types, and the initial C API. Boolean polygon operations are the next major
piece.

## Workspace

- `crates/knipsa` — the safe Rust API;
- `crates/knipsa-ffi` — the C-compatible library and public header;
- `tests/c` — a C11 ABI smoke test;
- `fuzz` — fuzz targets for geometry inputs.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
./scripts/check-c-api.sh
./scripts/coverage.sh
```

The coverage command requires `cargo-llvm-cov`.

## License

Licensed under either of:

- Apache License, Version 2.0;
- MIT License.
