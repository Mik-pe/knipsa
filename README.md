# knipsa

[![CI](https://github.com/Mik-pe/knipsa/actions/workflows/ci.yml/badge.svg)](https://github.com/Mik-pe/knipsa/actions/workflows/ci.yml)

![A crab claw clipping a triangle](https://raw.githubusercontent.com/Mik-pe/knipsa/main/assets/knipsa-crab-clips-triangle.png)

`knipsa` is a Rust polygon toolkit for boolean operations, offsets, point
queries, and triangulation. It has a safe Rust API and a small C-compatible
library for applications written in other languages.

The current published release line is `0.2`; patch releases preserve its API.
The `main` branch is preparing the intentionally breaking `0.3` API documented
below. Knipsa remains pre-1.0, so later minor releases may also break APIs.

## Quick start

For the current `main` API:

```sh
cargo add knipsa --git https://github.com/Mik-pe/knipsa
```

The published `0.2` line remains available with `cargo add knipsa@0.2`; use its
versioned docs until `0.3` is released. Projects using current `main` with
`geo-types` can enable zero-policy conversion helpers:

```sh
cargo add knipsa --git https://github.com/Mik-pe/knipsa --features geo-types
```

The `knipsa::geo_types` module converts `LineString` and `Polygon` values while
making ring closure explicit. It does not guess nesting across independent
polygons; use `build_polygons64` or `build_polygons_d` first when starting from
a flat collection of nested rings. `build_polygons64` classifies topology
exactly across the complete `i64` coordinate domain.

Enable `serde` to serialize and deserialize points, rectangles, enums,
options, resource limits, and structured errors. Since paths are ordinary
vectors, `Path64`, `PathD`, and triangle collections then work automatically:

```sh
cargo add knipsa --git https://github.com/Mik-pe/knipsa --features serde
```

Serialized enum variants use explicit `snake_case` names.

Boolean operations work directly on ordinary Rust vectors:

```rust
use knipsa::{PointD, intersection_path_d};

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

    let result = intersection_path_d(&subject, &clip)?;

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

Each coordinate/cardinality combination has one canonical name; the `0.x`
API intentionally does not retain duplicate aliases. Integer helpers use the
`64` suffix where a suffix is needed, while floating-point helpers use `_d`.
The configurable Boolean entry points are `boolean_op` and `boolean_op_d`.
Both take the same generic `BooleanRequest`, require a deterministic
`ComplexityLimits` budget, and return `BooleanOutput` with separate `closed`
and `open` path collections. Only subjects may be open; clips are closed.

| Need | Use |
| --- | --- |
| Two individual rings | `intersection_path`, `union_path`, `difference_path`, `xor_path` and `_d` variants |
| Ring collections with EvenOdd | `intersection`, `union`, `difference`, `xor` and `_d` variants |
| Exact integer polygon and open-path booleans | `boolean_op` with `Point64` |
| Fractional polygon and open-path booleans | `boolean_op_d` with `PointD` |
| Clean self-intersections or internal boundaries | `simplify_paths64` or `simplify_paths_d` |
| Clip paths to an axis-aligned rectangle | `clip_to_rect64` or `clip_to_rect_d` |
| Remove redundant collinear vertices | `trim_collinear64` or `trim_collinear_d` |
| Locate a malformed path or coordinate | `validate_paths64_located` or `validate_paths_d_located` |
| One polygon or polyline offset | `offset_path64` or `offset_path_d` |
| Polygon or polyline offset collections | `offset_paths64` or `offset_paths_d` |
| A single polygon's triangles | `triangulate_path64` or `triangulate_path_d`, with explicit limits |
| Multiple rings, holes, or islands | `triangulate64` or `triangulate_d`, with explicit limits |
| Explicit polygon and hole ownership | `build_polygons64` or `build_polygons_d` |
| Integer point-in-polygon queries | `point_in_polygon` |

Integer operations preserve integer coordinates. The floating-point boolean
API accepts finite `f64` values and computes intersections exactly before
returning `f64` coordinates. The convenience operations use the
orientation-independent `EvenOdd` rule; use `boolean_op`/`boolean_op_d` when a
different fill rule is required.

Offset options have named constructors for the common modes:

```rust
use knipsa::{EndType, JoinType, OffsetOptions, offset_path_d, offset_paths_d};

# fn example(ring: &knipsa::PathD, lines: &[knipsa::PathD]) -> Result<(), knipsa::Error> {
let expanded = offset_path_d(ring, 4.0, OffsetOptions::polygon(JoinType::Round))?;
let stroke = offset_paths_d(
    lines,
    2.0,
    OffsetOptions::polyline(JoinType::Round, EndType::Round).with_arc_tolerance(0.01),
)?;
# let _ = (expanded, stroke);
# Ok(())
# }
```

Closed offsets first try a certified contour forest. Simple generated rings
with no boundary contact are merged by containment and integer winding.
Touching, overlapping, self-crossing, oversized, or numerically ambiguous
contours fall back to the exact `NonZero` Boolean cleanup, so callers do not
need to choose a fast path or an epsilon policy.

## Geometry conventions

- Closed Boolean subjects, clips, and triangulation inputs are rings. Boolean
  subjects may additionally be open polylines; clips are always closed. Do not
  repeat a ring's first point at the end; normalization removes it if present.
- Boolean output separates closed rings from open polylines. Intersection keeps
  open fragments inside clips; difference and XOR keep fragments outside
  clips; union keeps fragments outside every filled closed subject and clip.
  Open paths neither fill regions nor interact with one another.
- Empty paths and empty path collections are valid and represent no geometry.
- Coordinates use the ordinary Cartesian convention: positive signed area is
  counter-clockwise winding.
- `EvenOdd` uses crossing parity. `NonZero`, `Positive`, and `Negative` use
  winding direction.
- Offset `Polygon` mode handles closed rings. `Joined`, `Butt`, `Square`, and
  `Round` handle open polylines.
- Triangulation returns counter-clockwise triangles and supports nested holes
  and islands.
- Polygon builders turn flat ring collections into counter-clockwise outer
  rings with owned clockwise holes; nested islands become separate polygons.
- Every Boolean, polygon-builder, and triangulation request is bounded by
  `ComplexityLimits`;
  `DEFAULT` bounds path count, total vertices, and conservative candidate
  intersection pairs before quadratic validation begins. There is no
  unbounded public path.

Geometry operations return `knipsa::Error`, whose structured `LimitExceeded`
variant distinguishes exceeded resource budgets from bad paths, non-finite
coordinates, overflow, and topology failures without a panic. Applications
that need input locations can run the `_located`
collection validators first and receive `PathValidationError` with stable path
and optional point indices.

## C and other languages

The `knipsa-ffi` crate exposes the closed-path operations and other core
geometry operations through a C ABI; open-subject Boolean clipping is currently
available only through the safe Rust API. The 0.2 ABI is preserved across 0.2.x
patch releases; pre-1.0 minor releases may break it.
The public header is
[`crates/knipsa-ffi/include/knipsa.h`](https://github.com/Mik-pe/knipsa/blob/main/crates/knipsa-ffi/include/knipsa.h),
and the ownership and null-pointer rules are documented in
[`docs/ffi.md`](https://github.com/Mik-pe/knipsa/blob/main/docs/ffi.md).

Outputs allocated by the FFI must be released with the matching
`knipsa_free_paths64` or `knipsa_free_paths_d` function. Input memory remains
owned by the caller. Initialize output slots with `KNIPSA_PATHS64_INIT` or
`KNIPSA_PATHS_D_INIT`; configure offsets with
`KNIPSA_OFFSET_OPTIONS_INIT`. Use
[`examples/quickstart.c`](https://github.com/Mik-pe/knipsa/blob/main/examples/quickstart.c)
as a complete C11 example.

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
./scripts/fuzz-replay.sh
make conformance-integer
make conformance-offset
make conformance-triangulation
make release-check
```

The coverage command requires `cargo-llvm-cov`; deterministic fuzz replay
requires `cargo-fuzz` and the nightly Rust toolchain.

See [`CHANGELOG.md`](https://github.com/Mik-pe/knipsa/blob/main/CHANGELOG.md) for
release notes,
[`docs/migrating-0.2.md`](https://github.com/Mik-pe/knipsa/blob/main/docs/migrating-0.2.md)
for the intentionally breaking `0.1` to `0.2` API changes,
[`docs/migrating-0.3.md`](https://github.com/Mik-pe/knipsa/blob/main/docs/migrating-0.3.md)
for the deletion-first next-minor API changes,
[`docs/release-scope-0.2.md`](https://github.com/Mik-pe/knipsa/blob/main/docs/release-scope-0.2.md)
for the 0.2 compatibility contract and known gaps, and
[`docs/releasing.md`](https://github.com/Mik-pe/knipsa/blob/main/docs/releasing.md)
for the crate publication procedure.

## License

Licensed under either of:

- Apache License, Version 2.0;
- MIT License.
