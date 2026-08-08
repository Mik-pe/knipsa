# knipsa

`knipsa` is a Rust polygon toolkit for boolean operations, offsets, point
queries, and triangulation. The safe API works with ordinary Rust vectors and
returns [`crate::Error`] instead of panicking on malformed geometry.

## Quick start

```rust
use knipsa::{PointD, intersection_path_d};

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
# Ok::<(), knipsa::Error>(())
```

Use [`crate::intersection`] for exact integer output. The convenience
operations and single-ring `_path` variants use [`crate::FillRule::EvenOdd`];
use [`crate::boolean_op`] or [`crate::boolean_op_d`] when another fill rule is
required. Use
[`crate::offset_path_d`] for one polygon or polyline offset,
[`crate::offset_paths_d`] for offset collections, and [`crate::triangulate_d`]
for counter-clockwise triangles.

Services accepting untrusted geometry can use
[`crate::triangulate_d_with_limits`] or [`crate::triangulate64_with_limits`]
with [`crate::TriangulationLimits::DEFAULT`] to reject oversized requests
before quadratic intersection validation begins.

Use [`crate::validate_paths_d_located`] or
[`crate::validate_paths64_located`] when validation diagnostics must include
the failing path and coordinate indices.

The optional `serde` feature implements `Serialize` and `Deserialize` for the
public value types without adding Serde to the default dependency graph. Enum
variants use explicit `snake_case` wire names.

Coordinate-specific helper names use `64` for exact integer geometry and `_d`
for floating-point geometry. Each operation has one canonical public name;
the pre-1.0 API does not retain duplicate aliases.

The repository contains a longer runnable tour in
`crates/knipsa/examples/quickstart.rs`.
