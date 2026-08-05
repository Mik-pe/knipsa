# Benchmark protocol

Benchmarks use the real boolean result path. A benchmark that only measures
validation, decoding, or an error path is not a clipping performance result.

## Workloads

Use the same versioned workload set for every implementation:

- small convex and concave polygons;
- holes and multiple disconnected components;
- many horizontal edges and touching boundaries;
- pathological self-intersections and repeated vertices;
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

## Reproducibility

Every report records the Knipsa commit, reference versions, compiler/toolchain,
optimization flags, operating system, CPU, workload hash, warm-up count,
sample count, and the exact command used. No performance claim is accepted
from a single warm run or from different input sets.

Reference adapters run external pinned builds or package versions. They do not
copy foreign implementation code into the Knipsa crates.

The checked-in adapters currently exercise `martinez-polygon-clipping` 0.8.1
and Shapely 2.0.7 / GEOS 3.11.4. The GEOS adapter is a correctness and
end-to-end API reference; it is not a claim that Python wrapper timing equals a
native GEOS C API timing. A native-vs-native result needs its own adapter.

## Result policy

Correctness is a gate; speed is a report. A faster result that fails one
required conformance case is not a win. A slower result with complete,
reproducible correctness data remains useful engineering information.
