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

To compare the optimized Rust run with an adapter:

```sh
./scripts/run-conformance.sh benchmarks/workloads.json target/conformance
```

The adapters use three warm-up calls and 25 measured calls per case. Their
signatures compare canonical filled-region rings; raw ring order is not a
correctness criterion. The comparison is fail-closed: every adapter must emit
one header and exactly one valid record for every workload case, and any
reported adapter error or missing case fails the command.

To inspect one pair manually, pass the workload explicitly:

```sh
python3 scripts/compare-benchmark-results.py \
  --workload benchmarks/workloads.json \
  target/conformance/knipsa.jsonl \
  target/conformance/clipper2.jsonl
```

For deterministic multi-outline scaling checks, generate the 4/8/16/32-ring
overlap chains and run the same harness:

```sh
./scripts/generate-scale-workloads.py target/overlap-scale.json
./scripts/run-conformance.sh \
  target/overlap-scale.json target/conformance-overlap-scale "clipper2 geos"
```

The optional third argument selects reference adapters. These cases use the
non-zero fill rule, where Clipper2 and GEOS agree on their canonical filled
regions. Martinez 0.8.1 does not, so it is deliberately excluded from this
matrix rather than treated as a Knipsa regression.
The generated workload is kept under `target/`; the generator is the durable,
reviewable fixture.

Reference adapters are comparison tools only; they are not runtime
dependencies of knipsa.

## Offset matrix

Offsetting has a separate workload because round joins can represent the same
curve with different vertices. The comparator checks ring count, filled area,
and bidirectional boundary distance using per-case tolerances:

```sh
cargo bench -p knipsa --bench offset_workload -- --nocapture > target/knipsa-offset.jsonl
benchmarks/reference/run-clipper2-offset.sh > target/clipper2-offset.jsonl
python3 scripts/compare-offset-results.py benchmarks/offset-workloads.json \
  target/knipsa-offset.jsonl target/clipper2-offset.jsonl
```
