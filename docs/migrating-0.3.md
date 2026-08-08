# Migrating from 0.2 to 0.3

The next pre-1.0 minor release removes unbounded topology analysis and
makes polygon hole ownership explicit. These changes are intentionally
breaking; there are no compatibility wrappers.

## Topology complexity limits

Every Rust polygon-builder and triangulation call now takes
`ComplexityLimits` and returns the shared `Error` type:

```rust
use knipsa::{
    FillRule, ComplexityLimits, build_polygons_d, triangulate64, triangulate_d,
    triangulate_path64, triangulate_path_d,
};

# let paths64 = Vec::new();
# let paths_d = Vec::new();
# let path64 = Vec::new();
# let path_d = Vec::new();
let limits = ComplexityLimits::DEFAULT;
let _ = build_polygons_d(&paths_d, FillRule::EvenOdd, limits)?;
let _ = triangulate64(&paths64, FillRule::EvenOdd, limits)?;
let _ = triangulate_d(&paths_d, FillRule::EvenOdd, limits)?;
let _ = triangulate_path64(&path64, limits)?;
let _ = triangulate_path_d(&path_d, limits)?;
# Ok::<(), knipsa::Error>(())
```

Tune `ComplexityLimits::new` for the application's latency and memory
budget. The old `TriangulationLimits`, `TriangulationError`, unbounded
triangulation functions, and duplicate `*_with_limits` functions were removed;
do not recreate an unbounded path with maximum integer values.

## Polygon ownership and `geo-types`

Use `build_polygons64` or `build_polygons_d`, with the same limits used for
triangulation, to turn a flat ring collection
into canonical polygons whose holes are owned explicitly. Nested filled
islands become separate polygons.

With the `geo-types` feature, use these one-to-one conversions:

| Direction | Integer | Floating point |
| --- | --- | --- |
| `geo-types` to Knipsa | `polygon64_from_geo` | `polygon_d_from_geo` |
| Knipsa to `geo-types` | `geo_polygon_from_polygon64` | `geo_polygon_from_polygon_d` |

The old flat `paths*_from_polygon` and `polygon_from_paths*` helpers were
removed because they discarded the typed ownership boundary.

## Request validation and version reporting

The standalone `validate_request` and `validate_request_d` wrappers were
removed. Boolean operations validate their requests automatically; use
`validate_paths64` or `validate_paths_d` when validation is needed separately.

The Rust-only `API_VERSION` constant was also removed because Cargo already
owns the statically linked crate version. The C ABI keeps its header version
macros and `knipsa_version()` runtime function for dynamically linked callers.

## C callers

The C function signatures are unchanged. Both triangulation entry points use
the documented fixed default budgets and return
`KNIPSA_STATUS_INVALID_ARGUMENT` when a budget is exceeded.
