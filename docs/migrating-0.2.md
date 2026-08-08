# Migrating from 0.1 to 0.2

Version 0.2 deliberately removes duplicate pre-1.0 aliases and makes coordinate
types visible in helper names. Update both `knipsa` and `knipsa-ffi` together
when an application uses both crates.

## Rust names

Floating-point helpers use `_d`; integer helpers use `64` where a suffix is
needed. The configurable Boolean entry points are the exception on the integer
side: `boolean_op` is integer and `boolean_op_d` is floating point.

Common migrations include:

| 0.1 spelling | 0.2 spelling |
| --- | --- |
| `boolean_opd` | `boolean_op_d` |
| `validate_requestd` | `validate_request_d` |
| `validate_pathd` | `validate_path_d` |
| `normalize_pathd` | `normalize_path_d` |
| `triangulate_pathd` | `triangulate_path_d` |

The duplicate `offset_paths`, `triangulate_paths64`, and
`triangulate_paths_d` aliases no longer exist. Use `offset_paths_d`,
`triangulate64`, and `triangulate_d`, respectively. Compile errors should be
fixed by selecting the canonical operation from the table in the root README
rather than adding a local compatibility wrapper.

## Floating-point behavior

Floating-point operations reject NaN and infinite coordinates. Boolean
intersections are constructed exactly internally and converted back to `f64`
for output. Callers should still choose tolerances appropriate to their domain
when comparing returned floating-point coordinates.

## C consumers

Use the header shipped by `knipsa-ffi 0.2.0`; do not combine a 0.1 header with a
0.2 library. The header exposes `KNIPSA_VERSION_MAJOR`,
`KNIPSA_VERSION_MINOR`, and `KNIPSA_VERSION_PATCH`, while `knipsa_version()`
returns the linked library version at runtime.

Outputs remain library-owned. Release integer outputs with
`knipsa_free_paths64` and floating-point outputs with `knipsa_free_paths_d`.
