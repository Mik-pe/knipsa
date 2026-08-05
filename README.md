# knipsa

[![CI](https://github.com/Mik-pe/knipsa/actions/workflows/ci.yml/badge.svg)](https://github.com/Mik-pe/knipsa/actions/workflows/ci.yml)

![A crab claw clipping a triangle](https://raw.githubusercontent.com/Mik-pe/knipsa/main/assets/knipsa-crab-clips-triangle.png)

`knipsa` is a Rust polygon toolkit for boolean operations, offsets, point
queries, and triangulation. It has a safe Rust API and a small C-compatible
library for applications written in other languages.

The Rust API is currently `0.x`, so breaking changes are still possible before
the first stable release.

## Quick start

For a checkout dependency:

```sh
cargo add --git https://github.com/Mik-pe/knipsa knipsa
```

Boolean operations work directly on ordinary Rust vectors:

```rust
use knipsa::{PointD, intersection_d};

fn main() -> Result<(), knipsa::Error> {
    let subject = vec![
        PointD::new(0.0, 0.0),
        PointD::new(10.0, 0.0),
        PointD::new(10.0, 10.0),
        PointD::new(0.0, 10.0),
    ];
    let clip = vec![
        PointD::new(5.0, 5.0),
        PointD::new(15.0, 5.0),
        PointD::new(15.0, 15.0),
        PointD::new(5.0, 15.0),
    ];

    let result = intersection_d(std::slice::from_ref(&subject), std::slice::from_ref(&clip))?;

    assert_eq!(result.len(), 1);
    Ok(())
}
```

The same example, including offsets, triangulation, and point classification,
is runnable from the repository:

```sh
cargo run -p knipsa --example quickstart
```

## Choosing the API

| Need | Use |
| --- | --- |
| Common EvenOdd booleans | `intersection`, `union`, `difference`, `xor` and `_d` variants |
| Exact integer polygon booleans | `boolean_op` with `Point64` |
| Fractional coordinates | `boolean_opd` with `PointD` |
| Polygon or polyline offsets | `offset_paths64` or `offset_paths_d` |
| A single polygon's triangles | `triangulate_path64` or `triangulate_pathd` |
| Multiple rings, holes, or islands | `triangulate64` or `triangulate_d` |
| Integer point-in-polygon queries | `point_in_polygon` |

Integer operations preserve integer coordinates. The floating-point boolean
API accepts finite `f64` values and computes intersections exactly before
returning `f64` coordinates. The convenience operations use the
orientation-independent `EvenOdd` rule; use `boolean_op`/`boolean_opd` when a
different fill rule is required.

## Geometry conventions

- Boolean and triangulation inputs are closed rings. Do not repeat the first
  point at the end; if it is repeated, normalization removes it.
- Empty paths and empty path collections are valid and represent no geometry.
- Coordinates use the ordinary Cartesian convention: positive signed area is
  counter-clockwise winding.
- `EvenOdd` uses crossing parity. `NonZero`, `Positive`, and `Negative` use
  winding direction.
- Offset `Polygon` mode handles closed rings. `Joined`, `Butt`, `Square`, and
  `Round` handle open polylines.
- Triangulation returns counter-clockwise triangles and supports nested holes
  and islands.

Every fallible operation returns `knipsa::Error`, so callers can handle bad
paths, non-finite coordinates, overflow, and topology failures without a
panic.

## C and other languages

The `knipsa-ffi` crate exposes the same operations through a stable C ABI. The
public header is [`crates/knipsa-ffi/include/knipsa.h`](crates/knipsa-ffi/include/knipsa.h),
and the ownership and null-pointer rules are documented in
[`docs/ffi.md`](docs/ffi.md).

Outputs allocated by the FFI must be released with the matching
`knipsa_free_paths64` or `knipsa_free_paths_d` function. Input memory remains
owned by the caller. Initialize output slots with `KNIPSA_PATHS64_INIT` or
`KNIPSA_PATHS_D_INIT`; configure offsets with
`KNIPSA_OFFSET_OPTIONS_INIT`. Use
[`examples/quickstart.c`](examples/quickstart.c) as a complete C11 example.

## Workspace

- `crates/knipsa` — the safe Rust API and geometry engine;
- `crates/knipsa-ffi` — the C-compatible library and public header;
- `crates/knipsa/examples/quickstart.rs` — a runnable API tour;
- `examples/quickstart.c` — a runnable C11 API tour;
- `tests/c` — a C11 ABI smoke test;
- `fuzz` — fuzz targets for geometry inputs;
- `docs/` — API contracts, testing notes, licensing, and benchmarks.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
./scripts/check-c-api.sh
./scripts/coverage.sh
```

The coverage command requires `cargo-llvm-cov`.

## License

Licensed under either of:

- Apache License, Version 2.0;
- MIT License.
