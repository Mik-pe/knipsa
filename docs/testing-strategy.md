# Testing strategy

The goal is not to make the test counter large. The goal is to make a wrong
polygon result difficult to ship.

## Layers

### 1. Contract and unit tests

Every public geometry type, checked arithmetic path, validation error, enum,
and FFI status must have deterministic tests. These are the tests covered by
the initial 100% coverage gate.

### 2. Feature contract tests

Offsets cover closed polygons, open polylines, every join and cap family,
positive and negative deltas, and the cleanup of concave/self-overlapping
outlines. Triangulation covers simple rings, holes, nested islands, all fill
rules, winding normalization, and rejection of intersecting paths.

### 3. Multi-reference corpus tests

Run a versioned corpus through the reference matrix in
[`reference-matrix.md`](reference-matrix.md). The first mandatory profile is
the shared integer closed-polygon profile; feature-specific cases are gated
separately. Each case gets a stable identifier and an expected semantic result,
not merely a count or an area. Run the local gate with
`./scripts/run-conformance.sh`; it requires a complete record set from
Boost.Geometry, Clipper2, geo/iOverlay, GEOS, JTS, and Martinez for the shared
OGC-valid profile and fails on adapter errors or missing cases. GEOS and JTS
are reported separately but count as one algorithm lineage. Self-crossing
EvenOdd fixtures run in a separate Clipper2 plus geo/iOverlay profile because
OGC-oriented adapters do not promise those input semantics.

The exact integer gate is `make conformance-integer`. It compares the public
`boolean_op` path with Clipper2 `Paths64`, preserves JSON integers beyond
2^53, and permits no coordinate or area tolerance. The benchmark harness,
calibration, raw-ring protocol, and comparator are shared with the floating
profile instead of being reimplemented for integers.

Feature gates use their own semantic comparators. `make conformance-offset`
checks region area and bidirectional boundary distance against Clipper2.
`make conformance-open` checks 22 exact integer Boolean cases against
Clipper2's native open-subject API. It validates the separate closed and open
outputs with atomic edges: closed boundary direction is irrelevant, while
open direction and edge multiplicity are part of the contract. Collinear
subdivision alone is not a mismatch.
`make conformance-triangulation` checks 12 integer cases against both Clipper2
and the independent `geo`/Spade constrained-Delaunay family. It accepts
different internal diagonals only after proving non-degenerate,
interior-disjoint triangles, exact total area, and complete input-boundary
reconstruction.
`make conformance-triangulation-d` separately checks eight scale-normalized
floating-point cases against `geo`/Spade, including tiny, huge, fractional,
thin, holed, and translated inputs.

### 4. Algebraic property tests

For bounded integer coordinates and valid closed paths, test properties such
as:

- union and XOR commutativity;
- intersection and union idempotence;
- difference with an empty set;
- intersection with an empty set;
- XOR self-cancellation;
- output rings are closed by convention and contain no accidental duplicate
  vertices;
- area and containment agree with the returned topology.

### 5. Differential tests

Run the same serialized cases through every required reference adapter and
Knipsa. Adapters report raw output rings; one fail-closed comparator validates
ring counts and coordinates, then derives canonical filled regions and
topology. Any exception, hang, overflow, non-finite result, malformed output,
or unexplained mismatch is a failing case saved as a regression fixture or a
documented semantic split.

### 6. Fuzzing and adversarial geometry

Fuzz malformed inputs, repeated points, horizontal edges, touching rings,
self-intersections, extreme coordinates, very small features, and operations
that create nested holes. `./scripts/fuzz-replay.sh` replays every checked-in
seed without mutating the corpus; longer exploratory campaigns are separate.
Integer and floating-point boolean targets assert output validity,
adjacent-vertex normalization, and operand-order invariance for commutative
operations.

Budgeted triangulation tests also prove that path, vertex, and conservative
edge-pair limits reject requests during linear preflight, before pairwise edge
validation or backend allocation.

### 7. FFI tests

Compile a C smoke client against the public header, exercise null/empty/valid
inputs, and verify exported symbols. Add language-level clients for at least
Python, Go, and one managed language before stabilizing the first release.

### 8. Benchmarks

Follow [`benchmarking.md`](benchmarking.md) for representative small, medium,
pathological, and high-vertex cases. Record compiler, optimization flags, CPU,
coordinate type, operation, input hash, output hash, and allocation behavior.
A speed claim requires a report that can be rerun from the repository.

## Release gate for the first clipping kernel

The first kernel is not release-ready until all of these are true:

- every required profile in the reference matrix passes with no silent skips;
- differential tests pass against all pinned references, with documented
  intentional semantic differences;
- property tests and fuzz replay cases pass;
- no panic, undefined behavior, or unbounded allocation is found on malformed
  FFI input;
- owned Rust code has 100% line, function, and branch coverage or an explicit
  reviewed exclusion;
- the FFI has a versioning policy and tested ownership rules;
- benchmarks are checked in and show the actual trade-offs.
