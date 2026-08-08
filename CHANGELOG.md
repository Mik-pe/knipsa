# Changelog

All notable changes to knipsa are documented here. The project follows
Semantic Versioning, with the usual allowance for breaking changes between
minor releases before `1.0`.

## [Unreleased]

### Added

- `build_polygons64` and `build_polygons_d` convert flat nested rings into
  strongly typed `Polygon64` and `PolygonD` values with canonical winding and
  explicit hole ownership. Integer nesting predicates are exact across the
  complete `i64` domain, and integer triangulation reuses them before adapting
  grouped vertices to the backend.

### Changed

- Polygon builders and all four Rust triangulation functions now require the
  same `ComplexityLimits` and return the same `Error`; structured resource
  preflight cannot be bypassed accidentally.
- The optional `geo-types` conversions now exchange `Polygon64` and `PolygonD`
  values instead of flattening exterior and interior rings into paths.
- `orientation` now returns `Orientation` directly because its checked `i128`
  path and `BigInt` fallback make every `Point64` input total.

### Removed

- `TriangulationLimits`, `TriangulationResource`, `TriangulationError`, the
  unbounded Rust triangulation path, and the duplicate `*_with_limits` family.
- The flat `geo-types` polygon helper family, replaced by conversions that
  preserve explicit hole ownership.

## [0.2.1] - 2026-08-08

### Changed

- The minimum supported Rust version is now 1.97.
- Floating-point triangulation now evaluates topology in a shared normalized
  coordinate frame, making results invariant across practical scales and
  translations while preserving the caller's original coordinates.
- The triangulation conformance profile now validates every case against both
  Clipper2 and the independent `geo`/Spade constrained-Delaunay implementation.
- A separately gated floating-point triangulation matrix now covers scale and
  translation from `1e-12` through `1e12` against normalized `geo`/Spade.
- New opt-in bounded triangulation entry points preflight path, vertex, and
  edge-pair budgets for services that accept untrusted geometry.
- The optional `geo-types` feature provides explicit `LineString` and `Polygon`
  conversions for integer and floating-point paths.
- Located collection validators now return `PathValidationError` with the
  failing path and optional vertex index without changing existing errors.
- The optional `serde` feature supports round-trip serialization for public
  points, rectangles, enums, options, limits, and structured errors. Enum
  variants use explicit `snake_case` names in serialized formats.

## [0.2.0] - 2026-08-08

### Added

- Floating-point Boolean operations with exact intersection construction and
  finite-coordinate validation.
- Polygon and open-polyline offsets with square, bevel, round, and miter joins
  plus joined, butt, square, and round endpoint modes.
- Integer and floating-point triangulation for rings, holes, and islands.
- Rectangle clipping, path simplification, collinear trimming, translation,
  orientation, and floating-point point/path validation helpers.
- C ABI coverage for integer and floating-point validation, Boolean,
  simplification, rectangle clipping, offset, and triangulation operations.
- Reproducible floating-point, integer, offset, and triangulation reference
  matrices, deterministic fuzz replay, and full line/function/branch coverage
  gates.

### Changed

- Floating-point helpers now consistently use the `_d` suffix, including
  `boolean_op_d`, `normalize_path_d`, and `triangulate_path_d`.
- Each public operation now has one canonical name; duplicate compatibility
  aliases from `0.1` were removed.
- Boolean dispatch, certified specializations, and exact fallbacks now share a
  single internal routing boundary.
- The minimum supported Rust version is 1.85 and the workspace uses Rust 2024.

### Compatibility

- This is an intentionally breaking pre-1.0 release. See
  [`docs/migrating-0.2.md`](docs/migrating-0.2.md).
- The safe Rust crate and C ABI crate are released together at version 0.2.0.
- Compatible performance improvements may ship in 0.2.x patch releases; the
  scope and deferred capabilities are recorded in
  [`docs/release-scope-0.2.md`](docs/release-scope-0.2.md).

[Unreleased]: https://github.com/Mik-pe/knipsa/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/Mik-pe/knipsa/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/Mik-pe/knipsa/compare/v0.1.1...v0.2.0
