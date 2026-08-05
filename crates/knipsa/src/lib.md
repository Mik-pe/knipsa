# knipsa

`knipsa` is a Rust polygon toolkit for boolean operations, offsets, point
queries, and triangulation. The safe API works with ordinary Rust vectors and
returns [`crate::Error`] instead of panicking on malformed geometry.

## Quick start

```rust
use knipsa::{boolean_opd, BooleanRequestD, ClipType, FillRule, PointD};

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

let result = boolean_opd(BooleanRequestD::new(
    std::slice::from_ref(&subject),
    std::slice::from_ref(&clip),
    ClipType::Intersection,
    FillRule::EvenOdd,
))?;

assert_eq!(result.len(), 1);
# Ok::<(), knipsa::Error>(())
```

Use [`crate::boolean_op`] with [`crate::Point64`] when exact integer output is
required. Use [`crate::offset_paths_d`] for polygon or polyline offsets and
[`crate::triangulate_d`] for counter-clockwise triangles.

The repository contains a longer runnable tour in
`crates/knipsa/examples/quickstart.rs`.
