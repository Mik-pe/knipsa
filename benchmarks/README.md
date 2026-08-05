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

To compare the optimized Rust run with an adapter:

```sh
cargo bench -p knipsa --bench workload -- --nocapture > target/knipsa.jsonl
benchmarks/reference/run-geos.sh benchmarks/workloads.json > target/geos.jsonl
python3 scripts/compare-benchmark-results.py target/knipsa.jsonl target/geos.jsonl
```

The adapters use three warm-up calls and 25 measured calls per case. Their
signatures compare canonical filled-region rings; raw ring order is not a
correctness criterion.

Reference adapters are comparison tools only; they are not runtime
dependencies of knipsa.
