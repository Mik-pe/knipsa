# Benchmarks

The workload file is shared by every adapter. It is intentionally small and
readable so a failing case can become a permanent regression fixture.

The Martinez adapter is pinned to `martinez-polygon-clipping` 0.8.1. Install
its dependencies in an ignored target directory and run it with:

```sh
benchmarks/reference/run-martinez.sh benchmarks/workloads.json
```

The GEOS adapter uses the Shapely 2.0.7 wheel, which reports GEOS 3.11.4 on
the reference machine:

```sh
benchmarks/reference/run-geos.sh benchmarks/workloads.json
```

The native Clipper2 adapter builds the pinned C++ reference in an ignored
`target/` directory and keeps the timing inside the C++ call:

```sh
benchmarks/reference/run-clipper2.sh benchmarks/workloads.json
```

The checkout is pinned to revision
`f9c5eb6e14a59f6f5d65fbfb3564519a561cf4fd`. It is a benchmark dependency only
and is not linked into the Rust library.

The independent native C++ comparison uses Boost.Geometry 1.91.0. Its build
script downloads the official source archive into the ignored
`target/reference` tree, verifies the release SHA-256 digest, and compiles only
the checked-in adapter:

```sh
benchmarks/reference/run-boost.sh benchmarks/workloads.json
```

Boost and Clipper2 share the checked-in C++ workload reader, calibration loop,
JSON error handling, and raw-ring serialization code in
`reference/cpp/boolean_adapter.hpp`; their adapters contain only
library-specific conversion and overlay dispatch. Boost accepts the OGC-valid
EvenOdd profile plus homogeneous-winding NonZero/Positive/Negative inputs and
fails closed on invalid or mixed-winding input.

The native Rust comparison uses `geo` 0.33.1 with default features disabled.
Its current BooleanOps implementation delegates to `i_overlay`; the adapter is
therefore identified as `geo-i-overlay`, not as an independent Geo kernel. Its
isolated Cargo project has a checked-in lockfile and does not affect Knipsa's
dependencies or Rust 1.97 MSRV:

```sh
benchmarks/reference/run-geo.sh benchmarks/workloads.json
```

The Java topology comparison uses JTS Core 1.20.0. The build script downloads
the Maven Central artifact into the ignored `target/reference` tree, verifies
its pinned SHA-256 digest, and compiles the checked-in adapter with Java 17:

```sh
benchmarks/reference/run-jts.sh benchmarks/workloads.json
```

The adapter repairs each input contour with JTS `GeometryFixer` and measures
`OverlayNGRobust`; geometry construction and JVM startup are outside the timed
region. JTS and GEOS share the same algorithm lineage, so passing both is a
cross-language robustness check rather than two independent algorithm votes.

To compare the optimized Rust run with an adapter:

```sh
./scripts/run-conformance.sh benchmarks/workloads.json target/conformance
```

The adapters use three warm-up calls, calibrate a power-of-two batch to at
least 2 ms per sample, and collect 25 measured samples per case. Reported
latencies are per-operation batch averages, and every record includes its
`iterations_per_sample`. This prevents timer resolution and CPU ramp-up from
dominating sub-microsecond cases. Adapters serialize raw output rings; the one
Python comparator validates them and centrally derives quantization,
collinear cleanup, canonical orientation, nesting, area, and equivalent
self-touching decompositions. Raw ring order is not a correctness criterion.
The comparison is fail-closed: every adapter must emit one header and exactly
one valid record for every workload case, and any reported adapter error,
missing case, ring-count disagreement, malformed ring, or uncalibrated record
fails the command.

The floating-point smoke workload declares its coordinate and doubled-area
tolerances and contains only OGC-valid inputs so all six default references
are applicable. These profile-owned limits accommodate documented
representation differences without letting an adapter choose its own
correctness threshold; ring topology and canonical vertex counts must still
agree exactly.

To inspect one pair manually, pass the workload explicitly:

```sh
python3 scripts/compare-benchmark-results.py \
  --workload benchmarks/workloads.json \
  target/conformance/knipsa.jsonl \
  target/conformance/clipper2.jsonl
```

For deterministic scaling checks, generate overlap chains and disjoint grids
at 4/8/16/32/64/128 outlines plus paired convex intersection/XOR cases at
32/64/128/256 vertices, then run the same harness:

```sh
./scripts/generate-scale-workloads.py target/overlap-scale.json
./scripts/run-conformance.sh \
  target/overlap-scale.json target/conformance-overlap-scale "boost clipper2 geo"
```

The optional third argument selects reference adapters. Multi-outline cases
use homogeneous positive winding under `NonZero`; vertex-count cases use
`EvenOdd`. Boost includes its required multi-contour region construction in
the timed operation, because that work is needed to implement the contour-plus-
fill-rule contract. Martinez 0.8.1 is deliberately excluded because it does
not expose the same multi-contour fill contract.
The generated workload is kept under `target/`; the generator is the durable,
reviewable fixture.

Both self-crossing contours are kept in a separate pathological matrix because
OGC-validity-oriented engines can repair or reinterpret them instead of
applying contour fill rules. The pinned Clipper2 and geo/iOverlay adapters
share the explicit EvenOdd semantics used by this fixture:

```sh
./scripts/run-conformance.sh benchmarks/pathological-workloads.json \
  target/conformance-pathological "clipper2 geo"
```

An adapter disagreement on invalid OGC input is recorded as a semantic
difference; adapters are never majority-voted into an oracle. JTS 1.20.0 is
deliberately excluded from this profile: its `GeometryFixer` repair produces
one ring with canonical doubled area `480` for
`orthogonal-self-crossing-even-odd`, while Knipsa, Clipper2, and geo/iOverlay
agree on doubled area `352` under the declared contour-level EvenOdd rule.
Boost.Geometry is also excluded because its documented overlay precondition is
an OGC-valid polygon; the adapter rejects both cases instead of benchmarking
undefined behavior.
The comparator accepts a self-touching ring and an equivalent multi-ring
decomposition only when their complete undirected boundary edge multisets,
including multiplicity and collinear subdivisions, match. Under the declared
EvenOdd profile that edge multiset defines the same parity-filled region.

Reference adapters are comparison tools only; they are not runtime
dependencies of knipsa.

## Open Boolean matrix

Open-subject Boolean clipping has a separate exact integer workload and emits
closed polygon results separately from clipped open polylines:

```sh
make conformance-open
```

The 22 generated cases cover every Boolean operation and fill rule, boundary
contacts, direction, multiple paths, holes, combined closed/open output, and
coordinates beyond exact `f64` representation. The comparator atomizes
collinear subdivisions across both results. Closed edges are compared without
direction; open edges retain direction and multiplicity. Missing cases,
adapter errors, malformed integer coordinates, and uncalibrated samples fail
closed.

## Offset matrix

Offsetting has a separate workload because round joins can represent the same
curve with different vertices. The comparator checks ring count, filled area,
and bidirectional boundary distance using per-case tolerances:

```sh
make conformance-offset
```

## Triangulation matrix

The integer triangulation profile compares `triangulate64` with both Clipper2's
native `Paths64` Delaunay triangulator at the pinned revision and `geo` 0.33.1's
Spade 2.15.1 constrained-Delaunay implementation. The latter is an independent
Rust algorithm family rather than another Clipper-derived vote. Its 12 cases
cover convex and concave rings, holes, multiple holes, nested islands,
disconnected components, redundant collinear vertices, thin corridors,
negative coordinates, and a translated large-coordinate polygon:

```sh
make conformance-triangulation
```

The separate `triangulate_d` profile covers eight fractional, holed, thin,
translated, and uniformly scaled cases from `1e-12` through `1e12`:

```sh
make conformance-triangulation-d
```

It compares against a scale-normalized `geo`/Spade adapter because Clipper2's
`PathsD` triangulator clamps decimal precision to eight places and therefore
cannot represent the smallest cases. The adapter restores caller coordinates
inside each timed operation. The comparator evaluates area, overlap, and
boundary reconstruction in a shared unit frame with fixed normalized budgets.

Triangulations are not compared by internal diagonals. The fail-closed
comparator independently validates every result against the input region: all
triangles must be non-degenerate, calibrated, interior-disjoint, preserve the
exact doubled area, and reconstruct every outer and hole boundary. This lets
different valid triangulations match without reducing correctness to an area
or triangle-count check.

The five native Rust workload binaries share one benchmark module for warm-up,
power-of-two batch calibration, sampling, and percentile selection. Closed
Boolean, open Boolean, offset, and triangulation timing therefore use identical
measurement logic.
The three isolated `geo` reference binaries likewise share one reference-side
calibration module.
