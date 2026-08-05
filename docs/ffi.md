# FFI contract

The FFI crate is named `knipsa-ffi`. It exists to make the library callable from
languages that can consume a C-compatible shared library. This is a boundary
chosen for interoperability, not an attempt to reproduce Clipper's ABI.

## Rules

- Every exported function is `extern "C"` and has a stable `#[repr(C)]`
  argument/result type.
- Inputs are borrowed for the duration of the call and are never freed by
  knipsa.
- Status codes are returned instead of Rust panics or exceptions crossing the
  boundary.
- Null pointers are accepted only where the function's documentation says
  they represent an empty slice.
- Output ownership is explicit. `knipsa_boolean64` allocates a
  `KnipsaPaths64` result and `knipsa_free_paths64` releases it; callers must
  never free Rust memory with their platform allocator.
- `knipsa_boolean_d` provides the same boolean contract for finite C `double`
  coordinates, with `KnipsaPathsD` released by `knipsa_free_paths_d`.
- ABI additions are versioned. Existing fields and enum values are never
  repurposed.

The initial API contains validation, point-location, and integer and
floating-point boolean calls. Boolean output is an owned array of
borrowed-looking path descriptors; the descriptors and their point arrays are
both released by the single matching free function for that coordinate type.
