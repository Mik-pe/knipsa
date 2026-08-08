# Changelog

All notable changes to knipsa are documented here. The project follows
Semantic Versioning, with the usual allowance for breaking changes between
minor releases before `1.0`.

## [Unreleased]

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

[Unreleased]: https://github.com/Mik-pe/knipsa/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/Mik-pe/knipsa/compare/v0.1.1...v0.2.0
