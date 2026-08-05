# Benchmark results

This is the reproducible baseline for the versioned 18-case workload.

## Run metadata

- source commit: `35ad1be` (`Optimize convex containment queries`)
- date: 2026-08-05
- machine: Apple `Mac17,2`, arm64, macOS 26.5, 10 logical CPUs
- Rust: `rustc 1.96.0`, optimized `cargo bench` profile
- workload SHA-256: `04f1408c1b9231d1eef0d7b23c2b3b403e4c991bfebaef14083f415d1ea706dd`
- samples: 25 per case, 3 warm-ups
- references: Clipper2 2.0.1 (`f9c5eb6e14a59f6f5d65fbfb3564519a561cf4fd`), Shapely 2.0.7 / GEOS 3.11.4, and `martinez-polygon-clipping` 0.8.1

The GEOS numbers measure the pinned Shapely adapter, including its Python
boundary. They are useful as an end-to-end reference, not as a native GEOS C
API claim.

## Correctness

Knipsa matched the canonical filled-region signature in all 18/18 cases
against Clipper2, GEOS/Shapely, and Martinez. These results come from fresh
runs of each adapter after the optimization commit.

## Median latency

Values are microseconds. The ratio columns are reference latency divided by
Knipsa latency; values above `1.00x` favor Knipsa.

| Case | Knipsa | GEOS/Shapely | Martinez | GEOS / Knipsa | Martinez / Knipsa |
| --- | ---: | ---: | ---: | ---: | ---: |
| overlap-intersection | 3.209 | 39.958 | 29.917 | 12.45x | 9.32x |
| overlap-union | 3.583 | 39.542 | 36.375 | 11.04x | 10.15x |
| overlap-difference | 1.583 | 39.125 | 23.416 | 24.72x | 14.79x |
| overlap-xor | 1.666 | 39.125 | 25.917 | 23.48x | 15.56x |
| nested-hole | 1.375 | 36.416 | 14.792 | 26.48x | 10.76x |
| disjoint-union | 0.458 | 30.750 | 2.625 | 67.14x | 5.73x |
| edge-touch-union | 1.666 | 37.916 | 12.041 | 22.76x | 7.23x |
| vertex-touch-xor | 1.458 | 34.666 | 7.625 | 23.78x | 5.23x |
| concave-crossing | 3.791 | 42.125 | 29.500 | 11.11x | 7.78x |
| fractional-crossing | 1.375 | 38.667 | 27.042 | 28.12x | 19.67x |
| self-crossing-even-odd | 2.167 | 59.750 | 57.500 | 27.57x | 26.53x |
| many-horizontal-edges | 3.542 | 39.917 | 24.625 | 11.27x | 6.95x |
| contained-intersection | 1.167 | 35.584 | 17.917 | 30.49x | 15.35x |
| contained-xor | 1.250 | 35.667 | 9.709 | 28.53x | 7.77x |
| repeated-collinear-union | 1.709 | 37.250 | 13.875 | 21.80x | 8.12x |
| near-touch-union | 0.417 | 30.709 | 3.959 | 73.64x | 9.49x |
| high-vertex-intersection | 13.417 | 79.250 | 170.041 | 5.91x | 12.67x |
| high-vertex-xor | 18.458 | 79.458 | 91.834 | 4.30x | 4.98x |

Knipsa was faster in all 18/18 cases against both GEOS/Shapely and Martinez.
The geometric-mean reference/Knipsa ratios are 20.01x for GEOS/Shapely and
9.90x for Martinez.

## Native Clipper2 comparison

The Clipper2 adapter is built from the pinned C++ checkout by
`benchmarks/reference/build-clipper2.sh`. Its timings exclude JSON parsing and
the process boundary; they measure the native `BooleanOp` call. Values are
microseconds and use the same 25 samples and 3 warm-ups as the Rust run. A
ratio above `1.00x` in the last column favors Clipper2.

| Case | Knipsa | Clipper2 native | Clipper2 / Knipsa |
| --- | ---: | ---: | ---: |
| overlap-intersection | 3.209 | 1.167 | 0.36x |
| overlap-union | 3.583 | 1.250 | 0.35x |
| overlap-difference | 1.583 | 0.833 | 0.53x |
| overlap-xor | 1.666 | 1.042 | 0.63x |
| nested-hole | 1.375 | 1.125 | 0.82x |
| disjoint-union | 0.458 | 0.916 | 2.00x |
| edge-touch-union | 1.666 | 0.916 | 0.55x |
| vertex-touch-xor | 1.458 | 0.833 | 0.57x |
| concave-crossing | 3.791 | 1.084 | 0.29x |
| fractional-crossing | 1.375 | 0.791 | 0.58x |
| self-crossing-even-odd | 2.167 | 0.667 | 0.31x |
| many-horizontal-edges | 3.542 | 1.333 | 0.38x |
| contained-intersection | 1.167 | 0.709 | 0.61x |
| contained-xor | 1.250 | 0.917 | 0.73x |
| repeated-collinear-union | 1.709 | 0.959 | 0.56x |
| near-touch-union | 0.417 | 0.917 | 2.20x |
| high-vertex-intersection | 13.417 | 3.167 | 0.24x |
| high-vertex-xor | 18.458 | 4.833 | 0.26x |

Across this small workload, Clipper2 is about 1.86x faster than Knipsa on the
geometric mean and about 1.78x faster at the median ratio. Knipsa wins 2/18
cases. It still matches all 18 filled-region signatures; the remaining gap is
concentrated in arrangement construction for high-vertex inputs.

## Optimization pass

This pass adds a convex-ring containment index with logarithmic fan queries,
while retaining the bucketed ray-crossing fallback for concave and
self-intersecting paths. It also keeps the earlier X-interval sweep, borrowed
input paths, fixed hashing, and degree-one stitch fast path.

`target-cpu=native` was measured separately, but did not produce a reliable
improvement on this branch. The hot path is dominated by sorting, branching,
intersection predicates, and arrangement construction rather than a uniform
numeric loop, so the implementation remains stable and portable Rust.

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
