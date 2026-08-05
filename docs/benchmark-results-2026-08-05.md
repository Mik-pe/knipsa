# Benchmark results

This is the reproducible baseline for the versioned 18-case workload.

## Run metadata

- source commit: `96606be7230b21a637aa4a7a6a2e12474f06dadf`
- date: 2026-08-05
- machine: Apple `Mac17,2`, arm64, macOS 26.5, 10 logical CPUs
- Rust: `rustc 1.96.0`, optimized `cargo bench` profile
- workload SHA-256: `04f1408c1b9231d1eef0d7b23c2b3b403e4c991bfebaef14083f415d1ea706dd`
- samples: 25 per case, 3 warm-ups
- references: Shapely 2.0.7 / GEOS 3.11.4 and `martinez-polygon-clipping` 0.8.1

The GEOS numbers measure the pinned Shapely adapter, including its Python
boundary. They are useful as an end-to-end reference, not as a native GEOS C
API claim.

## Correctness

Knipsa matched the canonical filled-region signature in all 18/18 cases
against both references.

## Median latency

Values are microseconds. The ratio columns are reference latency divided by
Knipsa latency; values above `1.00x` favor Knipsa.

| Case | Knipsa | GEOS/Shapely | Martinez | GEOS / Knipsa | Martinez / Knipsa |
| --- | ---: | ---: | ---: | ---: | ---: |
| overlap-intersection | 9.000 | 39.666 | 31.166 | 4.41x | 3.46x |
| overlap-union | 9.625 | 38.709 | 36.208 | 4.02x | 3.76x |
| overlap-difference | 8.458 | 38.792 | 24.916 | 4.59x | 2.95x |
| overlap-xor | 9.541 | 40.167 | 26.083 | 4.21x | 2.73x |
| nested-hole | 8.916 | 36.833 | 16.291 | 4.13x | 1.83x |
| disjoint-union | 1.792 | 31.292 | 2.666 | 17.46x | 1.49x |
| edge-touch-union | 8.875 | 36.708 | 11.334 | 4.14x | 1.28x |
| vertex-touch-xor | 7.750 | 36.125 | 7.875 | 4.66x | 1.02x |
| concave-crossing | 11.917 | 45.208 | 29.292 | 3.79x | 2.46x |
| fractional-crossing | 7.458 | 39.625 | 23.584 | 5.31x | 3.16x |
| self-crossing-even-odd | 5.000 | 61.334 | 31.083 | 12.27x | 6.22x |
| many-horizontal-edges | 12.250 | 40.917 | 56.709 | 3.34x | 4.63x |
| contained-intersection | 7.125 | 36.125 | 9.500 | 5.07x | 1.33x |
| contained-xor | 8.042 | 36.417 | 10.625 | 4.53x | 1.32x |
| repeated-collinear-union | 8.958 | 37.917 | 15.208 | 4.23x | 1.70x |
| near-touch-union | 1.541 | 31.500 | 1.792 | 20.44x | 1.16x |
| high-vertex-intersection | 38.542 | 82.667 | 101.250 | 2.14x | 2.63x |
| high-vertex-xor | 87.791 | 81.375 | 96.000 | 0.93x | 1.09x |

Knipsa was faster in 17/18 cases against GEOS/Shapely and 18/18 against
Martinez. The high-vertex XOR case is the current optimization target: it is
slightly slower than the GEOS adapter and only narrowly ahead of Martinez.

## Reproduce

From the repository root:

```sh
mkdir -p target/reports
cargo bench -p knipsa --bench workload -- --nocapture > target/reports/knipsa.jsonl
benchmarks/reference/run-geos.sh benchmarks/workloads.json > target/reports/geos.jsonl
benchmarks/reference/run-martinez.sh benchmarks/workloads.json > target/reports/martinez.jsonl
python3 scripts/compare-benchmark-results.py target/reports/knipsa.jsonl target/reports/geos.jsonl
python3 scripts/compare-benchmark-results.py target/reports/knipsa.jsonl target/reports/martinez.jsonl
```
