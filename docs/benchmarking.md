# Benchmark protocol

Benchmarks use the real boolean result path. A benchmark that only measures
validation, decoding, or an error path is not a clipping performance result.

## Workloads

Use the same versioned workload set for every implementation:

- small convex and concave polygons;
- holes and multiple disconnected components;
- many horizontal edges and touching boundaries;
- repeated vertices in the shared valid profile;
- pathological self-intersections in a separately declared semantic profile;
- high-vertex real-world-shaped inputs;
- empty, disjoint, containment, and identical inputs.

Each workload has a stable input hash. Integer workloads are preferred for the
first comparison so that output comparison does not hide rounding differences.

## Measurements

Record these separately:

- input decoding and output encoding;
- the clipping operation itself;
- allocations and peak memory, when the adapter can measure them;
- output component/ring/vertex counts;
- operations per second, median latency, and tail latency;
- failures, timeouts, and output mismatches.

The headline comparison is kernel-only latency on the same machine and input.
End-to-end numbers are reported separately so serialization or FFI overhead
does not get mistaken for algorithm speed.

Each checked-in adapter calibrates a power-of-two operation batch to at least
2 ms, then records 25 per-operation batch averages. The batch size is emitted
as `iterations_per_sample`; single-call timings are not accepted for these
comparisons because timer resolution and CPU frequency ramp-up can dominate
the smallest workloads.

Knipsa's Boolean, offset, and triangulation workload binaries call the same
Rust measurement module. Warm-up count, calibration ceiling, minimum sample
duration, sampling, and percentile selection therefore have one implementation
instead of three independently drifting loops.

## Reproducibility

Every report records the Knipsa commit, reference versions, compiler/toolchain,
optimization flags, operating system, CPU, workload hash, warm-up count,
sample count, and the exact command used. No performance claim is accepted
from a single warm run or from different input sets.

Reference adapters run external pinned builds or package versions. They do not
copy foreign implementation code into the Knipsa crates.

The checked-in adapters currently exercise Boost.Geometry 1.91.0 from its
checksum-pinned official archive, Clipper2 2.0.1 at the pinned source revision,
`geo` 0.33.1 backed by `i_overlay`, JTS Core 1.20.0,
`martinez-polygon-clipping` 0.8.1, and Shapely 2.0.7 / GEOS 3.11.4.
The GEOS adapter is a correctness and end-to-end API reference; it is not a
claim that Python wrapper timing equals a native GEOS C API timing. The
Clipper2 adapter keeps the operation inside native C++ and is the
native-vs-native comparison. Its single binary selects `PathsD` for the shared
floating profile and `Paths64` for `benchmarks/integer-workloads.json`; the
latter preserves decimal JSON integers exactly and compares with zero
tolerance. Knipsa's workload binary likewise dispatches the same calibration
loop to `boolean_op_d` or `boolean_op`, so coordinate profiles cannot drift in
timing methodology.
The Boost adapter is also native C++ and measures only its public overlay call
for single-ring inputs; input validation and output serialization remain
outside the timed region. When multiple contours must first be resolved under
a fill rule, that required Boost region construction is included in the timed
operation. Boost and Clipper2 share one C++ benchmark protocol so calibration
and transport logic cannot drift between those adapters. All adapters emit raw
rings and the Python comparator is the sole canonicalization implementation.
The `geo` adapter is also native Rust but measures its public BooleanOps call,
including conversion into the underlying iOverlay representation.
The JTS adapter constructs and repairs geometries before timing, then measures
`OverlayNGRobust` in a warmed Java 17 process. JVM startup and whitespace input
decoding are excluded. JTS and GEOS remain one algorithm lineage.

The integer triangulation comparison measures `triangulate64` against
Clipper2's native `Paths64` Delaunay entry point and `geo` 0.33.1 backed by the
independent Spade 2.15.1 constrained-Delaunay implementation. Correctness is
checked from the emitted triangles rather than internal diagonals: every
partition must have exact area and boundary, no degenerate triangles, and no
positive-area triangle overlap. Passing both remains a bounded pinned matrix,
not universal triangulation conformance.

The floating-point triangulation profile measures `triangulate_d` against a
scale-normalized `geo`/Spade adapter over eight cases spanning `1e-12` through
`1e12`. Both outputs are restored to caller coordinates and then independently
validated in a shared unit frame. Clipper2 `PathsD` is not included in this
profile because its triangulator clamps decimal precision to eight places.

## Result policy

Correctness is a gate; speed is a report. A faster result that fails one
required conformance case is not a win. A slower result with complete,
reproducible correctness data remains useful engineering information.
