# Benchmark results

This is the reproducible result for the versioned 18-case workload after the
strict-convex fast path optimization.

## Run metadata

- source commit: `1e8a835` (`Optimize strict convex boolean operations`)
- date: 2026-08-05
- machine: Apple `Mac17,2`, arm64, macOS 26.5, 10 logical CPUs
- Rust: `rustc 1.96.0`, optimized `cargo bench` profile
- workload SHA-256: `04f1408c1b9231d1eef0d7b23c2b3b403e4c991bfebaef14083f415d1ea706dd`
- samples: 25 per case, 3 warm-ups
- reported latency: median of three independent benchmark-process medians
- references: Clipper2 2.0.1 (`f9c5eb6e14a59f6f5d65fbfb3564519a561cf4fd`), Shapely 2.0.7 / GEOS 3.11.4, and `martinez-polygon-clipping` 0.8.1

The GEOS numbers measure the pinned Shapely adapter, including its Python
boundary. They are useful as an end-to-end reference, not as a native GEOS C
API claim.

## Correctness

Knipsa matched the canonical filled-region signature in all 18/18 cases
against Clipper2, GEOS/Shapely, and Martinez. Each reference was run three
times; every run matched all 18 cases.

## Median latency

Values are microseconds. Ratios above `1.00x` favor Knipsa.

| Case | Knipsa | GEOS/Shapely | Martinez | GEOS / Knipsa | Martinez / Knipsa |
| --- | ---: | ---: | ---: | ---: | ---: |
| overlap-intersection | 1.791 | 36.667 | 35.625 | 20.47x | 19.89x |
| overlap-union | 1.834 | 36.542 | 41.416 | 19.92x | 22.58x |
| overlap-difference | 1.750 | 36.167 | 28.708 | 20.67x | 16.40x |
| overlap-xor | 1.792 | 37.125 | 29.500 | 20.72x | 16.46x |
| nested-hole | 0.500 | 34.375 | 16.791 | 68.75x | 33.58x |
| disjoint-union | 0.500 | 29.417 | 2.667 | 58.83x | 5.33x |
| edge-touch-union | 1.709 | 34.208 | 12.583 | 20.02x | 7.36x |
| vertex-touch-xor | 1.459 | 32.875 | 8.333 | 22.53x | 5.71x |
| concave-crossing | 2.916 | 39.792 | 31.375 | 13.65x | 10.76x |
| fractional-crossing | 0.208 | 36.333 | 25.250 | 174.68x | 121.39x |
| self-crossing-even-odd | 1.875 | 56.709 | 33.709 | 30.24x | 17.98x |
| many-horizontal-edges | 3.041 | 37.542 | 49.333 | 12.35x | 16.22x |
| contained-intersection | 0.208 | 33.500 | 8.625 | 161.06x | 41.47x |
| contained-xor | 0.500 | 33.750 | 10.167 | 67.50x | 20.33x |
| repeated-collinear-union | 1.458 | 35.208 | 14.958 | 24.15x | 10.26x |
| near-touch-union | 0.500 | 28.916 | 1.875 | 57.83x | 3.75x |
| high-vertex-intersection | 1.792 | 73.417 | 123.875 | 40.97x | 69.13x |
| high-vertex-xor | 19.791 | 75.000 | 60.041 | 3.79x | 3.03x |

Knipsa was faster in all 18/18 cases against GEOS/Shapely and Martinez. The
geometric-mean ratios are 30.92x and 15.40x respectively.

## Native Clipper2 comparison

The Clipper2 adapter is built from the pinned C++ checkout by
`benchmarks/reference/build-clipper2.sh`. Its timings exclude JSON parsing and
the process boundary; they measure the native `BooleanOp` call. Ratios above
`1.00x` in the last column favor Knipsa.

| Case | Knipsa | Clipper2 native | Clipper2 / Knipsa |
| --- | ---: | ---: | ---: |
| overlap-intersection | 1.791 | 1.917 | 1.07x |
| overlap-union | 1.834 | 1.875 | 1.02x |
| overlap-difference | 1.750 | 1.416 | 0.81x |
| overlap-xor | 1.792 | 1.917 | 1.07x |
| nested-hole | 0.500 | 1.666 | 3.33x |
| disjoint-union | 0.500 | 1.667 | 3.33x |
| edge-touch-union | 1.709 | 1.625 | 0.95x |
| vertex-touch-xor | 1.459 | 1.458 | 1.00x |
| concave-crossing | 2.916 | 2.000 | 0.69x |
| fractional-crossing | 0.208 | 1.458 | 7.01x |
| self-crossing-even-odd | 1.875 | 1.166 | 0.62x |
| many-horizontal-edges | 3.041 | 2.291 | 0.75x |
| contained-intersection | 0.208 | 1.333 | 6.41x |
| contained-xor | 0.500 | 1.667 | 3.33x |
| repeated-collinear-union | 1.458 | 1.750 | 1.20x |
| near-touch-union | 0.500 | 1.667 | 3.33x |
| high-vertex-intersection | 1.792 | 5.292 | 2.95x |
| high-vertex-xor | 19.791 | 8.250 | 0.42x |

Knipsa won 11/18 cases; Clipper2 won 7/18. The geometric-mean latency ratio
was `1.54x` in Knipsa's favor, and the median per-case ratio was `1.07x`.
The remaining clear gap is high-vertex XOR, where Clipper2 measured about
2.4x faster on this workload.

## Optimization pass

This pass adds a strict-convex single-pair dispatch and a linear edge walk for
ordinary convex input. The walk supplies split parameters and containment
hints to a direct convex edge classifier; touching, collinear, ill-conditioned,
or otherwise uncertain cases use the existing conservative fallback.

The hot path also now borrows convex input in its containment index, stores the
usual split parameters inline, stops the walk after both boundaries have been
visited, and uses one compact outgoing-edge index during stitching. The added
regression tests cover the walk, degeneracies, fallback predicates, topology
failures, and quantized micro-intersections.

The repository gate remains strict: 44 Knipsa tests plus 8 FFI tests pass,
Clippy is clean, and `scripts/coverage.sh` reports 100% line, function, and
branch coverage.

## Reproduce

From the repository root:

```sh
mkdir -p target/reports
cargo bench -p knipsa --bench workload --profile release > target/reports/knipsa.jsonl
benchmarks/reference/run-geos.sh benchmarks/workloads.json > target/reports/geos.jsonl
benchmarks/reference/run-martinez.sh benchmarks/workloads.json > target/reports/martinez.jsonl
benchmarks/reference/run-clipper2.sh benchmarks/workloads.json > target/reports/clipper2.jsonl
python3 scripts/compare-benchmark-results.py target/reports/knipsa.jsonl target/reports/geos.jsonl
python3 scripts/compare-benchmark-results.py target/reports/knipsa.jsonl target/reports/martinez.jsonl
python3 scripts/compare-benchmark-results.py target/reports/knipsa.jsonl target/reports/clipper2.jsonl
```

For the checked-in table, repeat the four adapter runs three times and take
the median of each case's three reported medians.
