# Benchmark results

This is the reproducible result for the versioned 18-case workload after the
high-vertex convex XOR optimization.

## Run metadata

- source commit: `cc3c26c` (`Optimize high-vertex convex XOR`)
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
| overlap-intersection | 1.708 | 37.709 | 33.917 | 22.08x | 19.86x |
| overlap-union | 1.667 | 37.458 | 42.500 | 22.47x | 25.49x |
| overlap-difference | 1.542 | 38.125 | 27.667 | 24.72x | 17.94x |
| overlap-xor | 1.583 | 37.167 | 29.500 | 23.48x | 18.64x |
| nested-hole | 0.458 | 35.083 | 16.917 | 76.60x | 36.94x |
| disjoint-union | 0.458 | 29.292 | 2.833 | 63.96x | 6.19x |
| edge-touch-union | 1.542 | 33.875 | 12.625 | 21.97x | 8.19x |
| vertex-touch-xor | 1.333 | 32.458 | 8.250 | 24.35x | 6.19x |
| concave-crossing | 2.625 | 39.083 | 31.959 | 14.89x | 12.17x |
| fractional-crossing | 0.209 | 35.959 | 25.541 | 172.05x | 122.21x |
| self-crossing-even-odd | 1.667 | 57.250 | 33.083 | 34.34x | 19.85x |
| many-horizontal-edges | 2.625 | 37.708 | 30.167 | 14.36x | 11.49x |
| contained-intersection | 0.208 | 33.708 | 11.791 | 162.06x | 56.69x |
| contained-xor | 0.375 | 34.000 | 14.209 | 90.67x | 37.89x |
| repeated-collinear-union | 1.167 | 35.625 | 17.042 | 30.53x | 14.60x |
| near-touch-union | 0.417 | 29.084 | 1.959 | 69.75x | 4.70x |
| high-vertex-intersection | 1.541 | 77.958 | 110.333 | 50.59x | 71.60x |
| high-vertex-xor | 3.500 | 74.792 | 60.041 | 21.37x | 17.15x |

Knipsa was faster in all 18/18 cases against GEOS/Shapely and Martinez. The
geometric-mean ratios are 38.35x and 19.15x respectively.

## Native Clipper2 comparison

The Clipper2 adapter is built from the pinned C++ checkout by
`benchmarks/reference/build-clipper2.sh`. Its timings exclude JSON parsing and
the process boundary; they measure the native `BooleanOp` call. Ratios above
`1.00x` in the last column favor Knipsa.

| Case | Knipsa | Clipper2 native | Clipper2 / Knipsa |
| --- | ---: | ---: | ---: |
| overlap-intersection | 1.708 | 2.333 | 1.37x |
| overlap-union | 1.667 | 2.334 | 1.40x |
| overlap-difference | 1.542 | 1.709 | 1.11x |
| overlap-xor | 1.583 | 2.334 | 1.47x |
| nested-hole | 0.458 | 2.083 | 4.55x |
| disjoint-union | 0.458 | 2.083 | 4.55x |
| edge-touch-union | 1.542 | 2.084 | 1.35x |
| vertex-touch-xor | 1.333 | 1.792 | 1.34x |
| concave-crossing | 2.625 | 2.334 | 0.89x |
| fractional-crossing | 0.209 | 1.709 | 8.18x |
| self-crossing-even-odd | 1.667 | 1.458 | 0.87x |
| many-horizontal-edges | 2.625 | 2.792 | 1.06x |
| contained-intersection | 0.208 | 1.542 | 7.41x |
| contained-xor | 0.375 | 2.000 | 5.33x |
| repeated-collinear-union | 1.167 | 2.083 | 1.78x |
| near-touch-union | 0.417 | 2.084 | 5.00x |
| high-vertex-intersection | 1.541 | 6.791 | 4.41x |
| high-vertex-xor | 3.500 | 10.458 | 2.99x |

Knipsa won 16/18 cases; Clipper2 won 2/18. The geometric-mean latency ratio
was `2.32x` in Knipsa's favor, and the median per-case ratio was `1.63x`.
High-vertex XOR is now `3.0x` faster in Knipsa on the three-run median.

## Optimization pass

This pass keeps the strict-convex linear edge walk on the ordinary convex path,
avoids treating separated collinear edges as a degeneracy, seeds split-only
walks with the correct initial containment side, and stitches convex XOR edges
in source order before falling back to the general topology index. Convex
containment also rejects points outside a cached bounding box before its
logarithmic predicate.

The added regression tests cover the high-vertex rounded workload, exact-oracle
topology and area, collinear degeneracies, fallback predicates, and stitching
failures. The repository gate remains strict: 45 Knipsa tests plus 8 FFI tests
pass, Clippy is clean, and `scripts/coverage.sh` reports 100% line, function,
and branch coverage.

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
