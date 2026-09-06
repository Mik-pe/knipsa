# Exact integer spatial certification

## Decision

Replace the all-pairs simplicity certificate in `dispatch::try_direct_paths64` with an integer bounding-box hierarchy. Keep the existing checked integer narrow phase, operation semantics, public API, and general fallback. This is an original implementation of a conventional broad-phase technique, not a claim to have invented a new polygon-clipping algorithm.

The change accelerates one-sided integer union/difference/XOR and simplification of simple rings and separated collections. It does not replace the general arrangement noder, improve every clipping operation, or add GPU execution.

## Why this target

The old certificate compared every pair of edges even when their boxes were far apart. Its ring separation check also recomputed bounds inside a quadratic loop. Large sparse inputs therefore spent most of this path's time proving that impossible intersections did not exist.

The hierarchy stores bounds once, partitions by the widest axis using deterministic median selection, and prunes pairs of disjoint nodes. Bounds use `i64`; spans and twice-centers use `i128`, avoiding lossy floating conversion and signed addition overflow. Small collections of at most eight boxes use a stack array rather than allocating an index. Candidate pairs stream to the predicate instead of accumulating a potentially quadratic candidate vector.

Construction takes O(n log n) work and O(n) storage, with logarithmic tree depth. Query work depends on box overlap and can still be quadratic. No output-sensitive worst-case bound is claimed. Existing preflight limits remain unchanged, including their conservative quadratic work estimate; this PR does not make arbitrarily large sparse requests admissible.

## Correctness boundary

A segment lies inside its closed axis-aligned box. Disjoint boxes cannot hide an intersection; closed comparisons retain endpoint contacts and zero-width/height boxes. A node bounds every descendant, so pruning a disjoint node pair is conservative. The self/cross traversal partitions unordered pairs without duplication.

Surviving edge pairs retain checked `i128` determinant evaluation. Uncertain arithmetic, collinear overlap, self-crossings, and touching/overlapping ring boxes defer to the existing general pipeline. Non-adjacent edges sharing an endpoint now also defer: the previous endpoint-only predicate did not establish that such a ring was simple. Adjacent edges remain subject to the same overlap checks.

Tests compare candidate sets with brute-force box intersection over exhaustive small boxes, seeded random boxes, duplicates, degenerate boxes, full-range integer coordinates, and reversed order. Geometry cases cover contacts under every endpoint orientation, crossings, backtracking, overflow deferral, and separated rings.

## Measured result

Measured September 6, 2026, in [Actions run 34031808121](https://github.com/Mik-pe/knipsa/actions/runs/34031808121):

- Base: `f3860d0af69e7881488bac98f4010e15e3dceea5`.
- Measured head: `59286535186f9d0c2e272b5f4974e644430da584`.
- CPU model reported by the runner: AMD EPYC 7763 64-Core Processor. This is a single-process benchmark, not a claim to use 64 cores.
- Linux x86_64, Rust 1.98.1, LLVM 22.1.8, default release profile and features, empty `RUSTFLAGS`.
- The same harness and identical Cargo.lock were used for both revisions.

| Public API workload | Base ns/op | Head ns/op | Median paired base/head | Range across process pairs |
| --- | ---: | ---: | ---: | ---: |
| Rectangle, 4 vertices | 241.3 | 241.8 | 0.997x | 0.986–1.001x |
| Convex ring, 66 vertices | 48,266.6 | 6,341.5 | 7.611x | 7.600–7.674x |
| Convex ring, 258 vertices | 734,635.9 | 29,734.3 | 24.707x | 24.516–24.967x |
| Convex ring, 1,026 vertices | 11,613,745.0 | 135,088.9 | 85.971x | 85.192–86.630x |
| Convex ring, 2,050 vertices | 46,394,061.0 | 307,501.4 | 150.841x | 149.996–151.768x |
| 8 separated rectangles | 1,757.5 | 1,683.1 | 1.048x | 1.039–1.053x |
| 64 separated rectangles | 21,981.0 | 15,252.8 | 1.450x | 1.426–1.454x |
| 256 separated rectangles | 182,144.9 | 58,691.4 | 3.103x | 3.099–3.121x |
| Concave ring with disjoint collinear supports | 2,615.3 | 410.0 | 6.381x | 6.021–6.451x |
| Overlapping rectangle intersection control | 778.7 | 795.6 | 0.980x | 0.960–1.000x |

Five alternating base/head process pairs each measure 21 calibrated batches per case. Timing includes public validation, allocations, index construction, and output destruction. Input generation and result comparison are outside the timing interval. Each case is first checked against its analytically known complete integer geometry; the script also verifies identical canonical coordinate sequences between revisions. Equality is not reduced to area or vertex count.

The ratios are medians of paired process medians, not ratios of pooled samples. Their range is not a confidence interval. These are targeted synthetic cases on one virtualized CI host, not GIS-wide, cross-platform, or cross-library evidence. The overlapping control was about 2% slower in this run and is reported rather than omitted. No universal speedup or world-fastest claim follows from this table.

The artifact `direct-certification-59286535186f9d0c2e272b5f4974e644430da584` contains full timing samples, exact geometry signatures, iteration counts, execution order, machine metadata, and hashes. Artifact ZIP SHA-256: `d0c30578801c2d92d928b624ad62feec75d11441e34fe89afa0c291059b9724f`.

Reproduce with the checked-in harness against two trusted revisions:

```sh
python3 scripts/benchmark-direct-certification.py \
  --base f3860d0af69e7881488bac98f4010e15e3dceea5 \
  --head 59286535186f9d0c2e272b5f4974e644430da584 \
  --pairs 5 --output target/direct-certification.json
```

## Research and next experiments

Polygon processing research has not stopped. [PolygonTailor (2026)](https://www.mdpi.com/1999-4893/19/2/145) studies parallel Boolean operations in IC layouts. [GPU-Accelerated Algorithm for Polygon Reconstruction (2025)](https://www.mdpi.com/2076-3417/15/3/1111) addresses the reconstruction stage. Their published abstracts motivate experiments; this assessment does not reproduce their results or establish compatibility with knipsa's exact arithmetic contract. GPU reconstruction alone is not an exact end-to-end Boolean implementation.

[JTS monotone-chain documentation](https://locationtech.github.io/jts/javadoc/org/locationtech/jts/index/chain/MonotoneChain.html) describes a useful alternative to individual-edge indexing: contiguous chain subsets have cheaply recoverable bounds and support recursive intersection discovery. [Adaptive precision predicates](https://www.cs.cmu.edu/~quake/robust.html) provide another established way to avoid expensive arithmetic when the sign is certifiable. Exact predicates and exact construction of new vertices are separate obligations.

The next experiments should extend the existing [clipping and offsetting design](next-generation-clipping-offsetting.md), not introduce a second unused engine:

1. **General exact noding.** Apply spatial candidate generation or monotone chains to the general segment splitter, retaining the exact intersection construction and shared degeneracy policy. Measure sparse maps, dense overlaps, long crossing segments, coincident runs, holes, and adversarial boxes. Only generalize this private index when the second real consumer demonstrates compatible needs.
2. **Reusable geometry and memory.** Prototype an immutable prepared geometry plus a reusable execution workspace for repeated clipping against the same boundary. Keep integer and floating precision contracts distinct and make mutation invalidate or prevent reuse. Require end-to-end wins for 1, 10, and 1,000 queries, counting preparation and retained memory. Do not add public types solely for hypothetical workloads.
3. **Measured work limits.** Investigate budgets for visited node pairs, exact predicate work, arithmetic growth, output size, and memory instead of rejecting every request on a quadratic pair upper bound. Preserve deterministic denial-of-service protection and test early termination before relaxing the current API contract.
4. **Filtered arithmetic and parallel execution.** Profile predicate and exact-construction allocation costs first. Try independently certifiable components or CPU batches before GPU offload. GPU candidates must retain every possible contact, preserve exact fallback, and win with upload/readback and index-build costs included. GPU-resident bulk workloads and tiny CPU calls should have different execution decisions, not different correctness guarantees.

Before a best-in-class claim, run the existing reference adapters plus representative external workloads with pinned versions and aligned fill, boundary, open-path, and precision semantics. [Clipper2's documented integer/scaled-coordinate contract](https://www.angusj.com/clipper2/Docs/Overview.htm) is not interchangeable with exact binary-rational floating input. Publish correctness failures and regressions alongside latency distributions, allocation counts, memory use, and throughput on x86_64 and ARM64. A benchmark score must never conceal a different geometry problem.
