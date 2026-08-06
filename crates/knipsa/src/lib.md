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
use [`crate::boolean_op`] or [`crate::boolean_opd`] when another fill rule is
required. Use
[`crate::offset_path_d`] for one polygon or polyline offset,
[`crate::offset_paths_d`] for offset collections, and [`crate::triangulate_d`]
for counter-clockwise triangles.

The repository contains a longer runnable tour in
`crates/knipsa/examples/quickstart.rs`.
