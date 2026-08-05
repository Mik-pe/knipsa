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
- `knipsa_offset64` and `knipsa_offset_d` expose polygon and polyline offsets;
  the integer entry point returns rounded coordinates in `KnipsaPathsD` so
  joins and round caps are not truncated by the ABI.
- `knipsa_triangulate64` and `knipsa_triangulate_d` return each triangle as a
  three-point path and use the same fill-rule enum as boolean operations.
- ABI additions are versioned. Existing fields and enum values are never
  repurposed.

The API contains validation, point-location, boolean, offset, and triangulation
calls. Output is an owned array of
borrowed-looking path descriptors; the descriptors and their point arrays are
both released by the single matching free function for that coordinate type.

## Minimal C client

The header is self-contained. A null input pointer is valid when its count is
zero, so a union of one subject and no clips can be written like this:

```c
#include "knipsa.h"

#include <stdio.h>

int main(void) {
    const KnipsaPoint64 points[] = {
        {0, 0}, {10, 0}, {10, 10}, {0, 10},
    };
    const KnipsaPath64 subject = {points, 4};
    KnipsaPaths64 result = {0};

    const KnipsaStatus status = knipsa_boolean64(
        &subject, 1, NULL, 0,
        KNIPSA_CLIP_UNION, KNIPSA_FILL_EVEN_ODD,
        &result
    );
    if (status != KNIPSA_STATUS_OK) {
        fprintf(stderr, "knipsa failed: %s\n", knipsa_status_message(status));
        return 1;
    }

    printf("rings: %zu\n", result.path_count);
    knipsa_free_paths64(&result);
    return 0;
}
```

Compile the repository smoke client with:

```sh
./scripts/check-c-api.sh
```
