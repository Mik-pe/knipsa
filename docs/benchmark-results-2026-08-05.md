# Benchmark results

This is the reproducible baseline for the versioned 18-case workload.

## Run metadata

- source commit: `4087c8ada34d69ecd250d7944aee0cf218cc8e12`
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
against Clipper2, GEOS/Shapely, and Martinez.

## Median latency

Values are microseconds. The ratio columns are reference latency divided by
Knipsa latency; values above `1.00x` favor Knipsa.

| Case | Knipsa | GEOS/Shapely | Martinez | GEOS / Knipsa | Martinez / Knipsa |
| --- | ---: | ---: | ---: | ---: | ---: |
| overlap-intersection | 3.500 | 43.417 | 61.208 | 12.40x | 17.49x |
| overlap-union | 3.625 | 40.667 | 68.625 | 11.22x | 18.93x |
| overlap-difference | 3.167 | 40.125 | 51.750 | 12.67x | 16.34x |
| overlap-xor | 3.541 | 40.542 | 48.542 | 11.45x | 13.71x |
| nested-hole | 3.292 | 37.375 | 16.125 | 11.35x | 4.90x |
| disjoint-union | 0.625 | 31.750 | 4.542 | 50.80x | 7.27x |
| edge-touch-union | 3.333 | 36.958 | 23.708 | 11.09x | 7.11x |
| vertex-touch-xor | 2.916 | 36.125 | 16.000 | 12.39x | 5.49x |
| concave-crossing | 4.583 | 44.958 | 29.917 | 9.81x | 6.53x |
| fractional-crossing | 3.167 | 39.875 | 48.500 | 12.59x | 15.31x |
| self-crossing-even-odd | 2.125 | 59.500 | 60.583 | 28.00x | 28.51x |
| many-horizontal-edges | 5.000 | 41.666 | 28.417 | 8.33x | 5.68x |
| contained-intersection | 2.917 | 36.875 | 18.625 | 12.64x | 6.38x |
| contained-xor | 3.292 | 37.041 | 20.041 | 11.25x | 6.09x |
| repeated-collinear-union | 4.041 | 38.167 | 27.625 | 9.44x | 6.84x |
| near-touch-union | 0.667 | 32.000 | 1.792 | 47.98x | 2.69x |
| high-vertex-intersection | 30.666 | 82.375 | 92.125 | 2.69x | 3.00x |
| high-vertex-xor | 38.542 | 82.833 | 117.750 | 2.15x | 3.06x |

Knipsa was faster in all 18/18 cases against both GEOS/Shapely and Martinez.
The geometric-mean reference/Knipsa ratios are 11.71x for GEOS/Shapely and
7.78x for Martinez.

## Native Clipper2 comparison

The Clipper2 adapter is built from the pinned C++ checkout by
`benchmarks/reference/build-clipper2.sh`. Its timings exclude JSON parsing and
the process boundary; they measure the native `BooleanOp` call. Values are
microseconds and use the same 25 samples and 3 warm-ups as the Rust run. A
ratio above `1.00x` in the last column favors Clipper2.

| Case | Knipsa | Clipper2 native | Clipper2 / Knipsa |
| --- | ---: | ---: | ---: |
| overlap-intersection | 3.500 | 3.000 | 0.86x |
| overlap-union | 3.625 | 2.958 | 0.82x |
| overlap-difference | 3.167 | 2.208 | 0.70x |
| overlap-xor | 3.541 | 2.917 | 0.82x |
| nested-hole | 3.292 | 2.584 | 0.78x |
| disjoint-union | 0.625 | 2.625 | 4.20x |
| edge-touch-union | 3.333 | 2.625 | 0.79x |
| vertex-touch-xor | 2.916 | 2.333 | 0.80x |
| concave-crossing | 4.583 | 3.208 | 0.70x |
| fractional-crossing | 3.167 | 2.250 | 0.71x |
| self-crossing-even-odd | 2.125 | 1.875 | 0.88x |
| many-horizontal-edges | 5.000 | 3.625 | 0.72x |
| contained-intersection | 2.917 | 2.042 | 0.70x |
| contained-xor | 3.292 | 2.583 | 0.78x |
| repeated-collinear-union | 4.041 | 2.750 | 0.68x |
| near-touch-union | 0.667 | 2.625 | 3.94x |
| high-vertex-intersection | 30.666 | 8.959 | 0.29x |
| high-vertex-xor | 38.542 | 13.625 | 0.35x |

Across this small workload, Clipper2 is about 1.20x faster than Knipsa on the
geometric mean and about 1.27x faster at the median ratio. Knipsa wins 2/18
cases. It still matches all 18 filled-region signatures; the remaining gap is
small on this workload and is concentrated in arrangement construction.

## Reproduce

From the repository root:

```sh
mkdir -p target/reports
cargo bench -p knipsa --bench workload -- --nocapture > target/reports/knipsa.jsonl
benchmarks/reference/run-geos.sh benchmarks/workloads.json > target/reports/geos.jsonl
benchmarks/reference/run-martinez.sh benchmarks/workloads.json > target/reports/martinez.jsonl
benchmarks/reference/run-clipper2.sh benchmarks/workloads.json > target/reports/clipper2.jsonl
python3 scripts/compare-benchmark-results.py target/reports/knipsa.jsonl target/reports/geos.jsonl
python3 scripts/compare-benchmark-results.py target/reports/knipsa.jsonl target/reports/martinez.jsonl
python3 scripts/compare-benchmark-results.py target/reports/knipsa.jsonl target/reports/clipper2.jsonl
```
