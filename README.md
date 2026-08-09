# knipsa

[![CI](https://github.com/Mik-pe/knipsa/actions/workflows/ci.yml/badge.svg)](https://github.com/Mik-pe/knipsa/actions/workflows/ci.yml)

![A crab claw clipping a triangle](https://raw.githubusercontent.com/Mik-pe/knipsa/main/assets/knipsa-crab-clips-triangle.png)

Polygon booleans, offsets, point queries, and triangulation for Rust. Knipsa
uses ordinary vectors, returns structured errors, and has a small C API.

Boolean operations use specialized paths when their checks succeed and the
exact geometry kernel otherwise. Callers use the same API either way.

## Install

Install the current release from crates.io:

```sh
cargo add knipsa@0.3
```

Knipsa is pre-1.0. Patch releases preserve their API; minor releases may make
breaking changes.

## Quick start

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

Run the longer example for booleans, offsets, triangulation, and point
classification:

```sh
cargo run -p knipsa --example quickstart
```

## API guide

Integer names use `64`; floating-point names use `_d`. Each operation has one
public name per coordinate type.

| Task | Function |
| --- | --- |
| Boolean operation on two rings | `intersection_path`, `union_path`, `difference_path`, `xor_path` |
| Boolean operation on ring collections | `intersection`, `union`, `difference`, `xor` |
| Choose operation, fill rule, open subjects, or limits | `boolean_op` |
| Offset one path or a collection | `offset_path64`, `offset_paths64` |
| Triangulate one ring or nested rings | `triangulate_path64`, `triangulate64` |
| Build polygons with owned holes | `build_polygons64` |
| Clean or clip paths | `simplify_paths64`, `trim_collinear64`, `clip_to_rect64` |
| Validate with path and point indices | `validate_paths64_located` |
| Classify an integer point | `point_in_polygon` |

Use the `_d` form for `PointD` paths: for example `boolean_op_d`,
`offset_path_d`, and `triangulate_d`.

The short Boolean helpers use `EvenOdd`. `boolean_op` and `boolean_op_d`
accept a `BooleanRequest` when you need another fill rule, open subject lines,
or a custom `ComplexityLimits` budget. Their `BooleanOutput` keeps closed rings
and open lines separate.

Offsets use named options:

```rust
use knipsa::{EndType, JoinType, OffsetOptions, offset_path_d, offset_paths_d};

# fn example(ring: &knipsa::PathD, lines: &[knipsa::PathD]) -> Result<(), knipsa::Error> {
let expanded = offset_path_d(ring, 4.0, OffsetOptions::polygon(JoinType::Round))?;
let stroke = offset_paths_d(
    lines,
    2.0,
    OffsetOptions::polyline(JoinType::Round, EndType::Round)
        .with_arc_tolerance(0.01)
        .with_limits(knipsa::ComplexityLimits::DEFAULT),
)?;
# let _ = (expanded, stroke);
# Ok(())
# }
```

## Coordinates and geometry

- `Point64` represents exact integer geometry. Integer Boolean operations never
  round silently; a fractional result returns `NonIntegralResult`.
- `PointD` accepts finite `f64` coordinates. Boolean intersections are computed
  from their exact binary values before results are converted back to `f64`.
- Rings do not repeat their first point. A repeated closing point is removed
  during normalization.
- Positive signed area means counter-clockwise winding. Winding matters for
  `NonZero`, `Positive`, and `Negative`; it does not matter for `EvenOdd`.
- Boolean clips are closed. Subjects may also be open lines. Open lines do not
  fill an area or interact with one another.
- Triangles are returned counter-clockwise. Nested holes and islands are
  supported.

Boolean operations, offsets, polygon building, and triangulation have explicit
work limits. `ComplexityLimits::DEFAULT` is suitable for ordinary input;
services that accept untrusted geometry can set tighter limits. Limit failures
are reported separately from invalid paths, non-finite coordinates, overflow,
and topology errors.

## Optional integrations

Enable `geo-types` for conversions to and from `LineString` and `Polygon`:

```sh
cargo add knipsa@0.3 --features geo-types
```

The conversion helpers keep ring closure explicit. When starting with a flat
collection of nested rings, call `build_polygons64` or `build_polygons_d` before
converting to polygons.

Enable `serde` to serialize the public points, rectangles, options, enums,
limits, and errors. Paths and triangles then work through their standard
`Vec` and array implementations. Enum values use `snake_case`.

## C API

`knipsa-ffi` exposes closed-path booleans, offsets, triangulation, validation,
rectangle clipping, and point classification. Open-subject Boolean clipping is
currently Rust-only.

The public header is
[`crates/knipsa-ffi/include/knipsa.h`](https://github.com/Mik-pe/knipsa/blob/main/crates/knipsa-ffi/include/knipsa.h).
Ownership and null-pointer rules are in
[`docs/ffi.md`](https://github.com/Mik-pe/knipsa/blob/main/docs/ffi.md), and
[`examples/quickstart.c`](https://github.com/Mik-pe/knipsa/blob/main/examples/quickstart.c)
is a complete C11 example.

## Performance and correctness

The repository compares results before it reports timings. References are
pinned and used only by the benchmark tools, never by the library:

```sh
make conformance-integer
make conformance-open
make conformance-offset
make conformance-triangulation
make conformance-triangulation-d
```

These are finite workload sets, not universal speed or compatibility claims.
Results depend on the commit, compiler, machine, and input. The adapters,
measurement protocol, and workload coverage are documented in
[`benchmarks/README.md`](https://github.com/Mik-pe/knipsa/blob/main/benchmarks/README.md).

## Development

```sh
make check
make docs
make coverage
make fuzz-replay
make release-check
```

`coverage` needs `cargo-llvm-cov`. `fuzz-replay` needs `cargo-fuzz` and the
nightly Rust toolchain. The contributor rules are in
[`AGENTS.md`](https://github.com/Mik-pe/knipsa/blob/main/AGENTS.md).

For release history and migration notes, see
[`CHANGELOG.md`](https://github.com/Mik-pe/knipsa/blob/main/CHANGELOG.md),
[`docs/migrating-0.2.md`](https://github.com/Mik-pe/knipsa/blob/main/docs/migrating-0.2.md),
and
[`docs/migrating-0.3.md`](https://github.com/Mik-pe/knipsa/blob/main/docs/migrating-0.3.md).
The 0.3 guarantees and known limits are in
[`docs/release-scope-0.3.md`](https://github.com/Mik-pe/knipsa/blob/main/docs/release-scope-0.3.md).

## License

Apache-2.0 or MIT, at your choice.
