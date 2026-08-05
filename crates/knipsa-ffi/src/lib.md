# knipsa-ffi

`knipsa-ffi` exposes the polygon API through a small C-compatible ABI for
applications and language bindings that can load a C shared or static
library.

Inputs are borrowed for the duration of each call. Results are allocated by
knipsa and must be released with the matching `knipsa_free_paths64` or
`knipsa_free_paths_d` function. Initialize result slots with
`KNIPSA_PATHS64_INIT` or `KNIPSA_PATHS_D_INIT`; release a live result before
reusing its slot.

Offset calls use `KnipsaOffsetOptions`, normally initialized with
`KNIPSA_OFFSET_OPTIONS_INIT`. Status codes are returned for invalid pointers,
bad geometry, invalid options, arithmetic failures, and topology failures.

## Minimal C client

```c
#include "knipsa.h"

#include <stdio.h>

int main(void) {
    const KnipsaPoint64 points[] = {
        {0, 0}, {10, 0}, {10, 10}, {0, 10},
    };
    const KnipsaPath64 subject = {points, 4};
    KnipsaPaths64 result = KNIPSA_PATHS64_INIT;

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

The repository contains a complete C11 tour in `examples/quickstart.c` and a
contract reference in `docs/ffi.md`.
