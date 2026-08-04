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
- Output ownership will be explicit. A future allocated result must have a
  matching knipsa release function; callers must never free Rust memory with
  their platform allocator.
- ABI additions are versioned. Existing fields and enum values are never
  repurposed.

The initial API intentionally contains validation and point-location calls
only. Boolean and offset functions will be added once their Rust contracts are
implemented and tested.
