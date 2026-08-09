# Release scope for 0.3

Version 0.3 makes Knipsa's topology and resource contracts explicit across the
safe Rust API. It is an intentionally breaking pre-1.0 release.

## Guarantees

- Integer and floating-point Boolean operations use one generic bounded
  request and return separate closed and open path collections.
- Integer Boolean and polygon-topology predicates remain exact across the full
  `i64` coordinate domain; fractional integer results are reported rather than
  rounded.
- Boolean operations, offsets, polygon builders, and triangulation expose
  deterministic `ComplexityLimits` budgets.
- Certified specializations fail closed to the exact or general kernel when a
  precondition cannot be proven.
- `Polygon64` and `PolygonD` preserve explicit outer-ring and hole ownership,
  including through the optional `geo-types` conversions.
- `knipsa` and `knipsa-ffi` are released from the same commit and version. C
  ABI signatures remain compatible with 0.2; Rust calls through the FFI crate
  correctly expose raw-pointer validity as `unsafe`.
- Every checked-in external reference matrix must match semantically before a
  release. Timings never replace correctness checks.

## Known limits

- Open-subject Boolean clipping is available through the safe Rust API but not
  through the C ABI.
- Integer offsets use floating-point construction and round their final
  vertices; use the `_d` API when fractional offset coordinates matter.
- Triangulation uses `earcutr` after Knipsa validates, groups, and normalizes
  topology.
- The checked-in reference matrices are finite. They do not prove universal
  equivalence or performance leadership across all real-world geometry,
  machines, or toolchains.

## Patch policy

Compatible correctness, dispatch, allocation, and performance improvements may
ship in 0.3.x. Public Rust API or C ABI breaks require a later minor release.
