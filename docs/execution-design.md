# Execution design: exact topology, spatial candidates, conditional GPU offload

## Decision

Keep one geometry kernel and one public precision contract. Improve candidate
selection on the CPU before selecting a GPU backend. GPU is an experiment to
justify with hardware measurements, not a default selected by vertex count or
an alternative approximate geometry engine.

The follow-up to #14 introduces one shared closed-box hierarchy in `spatial.rs`.
The direct integer certificate uses it with native integer bounds. The general
closed-path noder uses it with exact coordinate ranks. `boolean/noding.rs` owns
candidate generation; the existing rational intersection, atomic-edge merging,
face winding, and ring tracing own geometry. No public API, dependency, feature
flag, or complexity limit is added.

The old noder was not an all-pairs implementation: it sorted by X and scanned
an active vector. Its pathological case is many long X intervals whose Y
intervals are separated. The new index prunes on both axes. Dense box overlap
can still require quadratic pair traversal; this is not a new worst-case
output-sensitive clipping algorithm.

## Exact ranks, not quantization

For each axis, sort the existing rational box endpoints by their exact order.
Equal coordinates receive the same rank and distinct coordinates receive
strictly increasing ranks. Then, for any two endpoint values `a` and `b`,
`a < b` if and only if `rank(a) < rank(b)`. Thus every closed-box overlap test
has exactly the same truth value before and after compression. Shared points,
zero-width boxes, tiny gaps, and enormous coordinates are not collapsed.

Only references to rationals are sorted; no BigInt coordinates are cloned by
the ranking stage. One coordinate buffer is reused for both axes. The number
of ranks is at most twice the number of allocated edges. Bounds and the index
use linear scratch storage. Construction and ranking take O(E log E) comparisons;
the cost of an exact comparison depends on the coordinate bit lengths.

Ranks are NOT coordinates for constructing intersections. A monotone nonlinear
mapping can change slopes and intersection locations. The pair visitor passes
only original edge IDs to `split_edge_pair`, which still computes with the
original rationals. Input edge order, sorted exact split parameters, fill rules,
coincident contributions, and output ordering retain their existing semantics.

Ranks are local to one request, not persistent vertex identities. A future
prepared geometry or GPU cache must merge coordinate orders correctly or rebuild
the ranks, and associate buffers with the request generation. Never compare ranks
from independent preparations or reuse stale edge IDs.

The index visits unordered pairs once and streams them directly to the exact
splitter, avoiding an O(K) candidate allocation. The previous X-only active
scan is removed rather than retained as another production backend.

The integer ring certificate also avoids redundant arithmetic at adjacent edges.
They already share an endpoint; a checked nonzero cross product proves that their
supporting lines have no second intersection. Collinear or overflowing cases
still defer. Non-adjacent contacts still require the complete intersection
contract. An exhaustive small-coordinate test compares this corner certificate
with the previous full predicate, including the closing edge.

## What should remain stable for callers

The existing convenience functions and `BooleanRequest` remain the API. A
caller should not need to select a hardware backend to get correct geometry.
A future prepared-geometry API must be immutable and explicitly own its cache;
a workspace must be reusable without retaining the previous request's result
or user geometry unexpectedly. Add either only after repeated-query workloads
justify preparation cost and retained memory. Do not publish placeholder types.

The existing preflight budget still conservatively charges possible edge pairs.
This change does not make million-edge sparse requests admissible under defaults.
Before relaxing that bound, charge visited nodes, exact predicates, arithmetic
growth, generated nodes, output size, and memory across the entire operation.
GPU batching does not remove these denial-of-service and memory obligations.

## GPU experiment boundary

WGSL's concrete numeric scalar types are `i32`, `u32`, `f32`, and `f16`.
There is no built-in arbitrary-precision rational type. This does not make exact
GPU geometry impossible, but it makes a direct shader translation of this
kernel a different, much larger project. [WGSL scalar types][wgsl]

The first hardware experiment should offload **box candidates only**:

1. Prepare exact ranks on the CPU. Build an explicitly packed descriptor of
   four `u32` bounds plus original edge IDs, after checked cardinality and device
   buffer-limit validation. The current Rust `i64`/`usize` structs are NOT a
   stable GPU buffer ABI and must not be uploaded by reinterpreting their bytes.
2. Traverse disjoint index tasks or spatial tiles, retaining closed interval
   comparisons. Every unordered pair needs one deterministic owner. Long edges
   must not be replicated through an unbounded number of tiles. Do not replace
   the index with an E-by-E brute-force shader and call that algorithmic progress.
3. Use bounded count/prefix-sum/write passes or bounded chunks for candidate
   output. Check all counters and allocations. Overflow, device loss, or partial
   execution must discard the whole candidate result, not truncate it.
4. Run the existing exact splitter and topology on original CPU coordinates.
   A failed device attempt may retry candidate generation on the CPU only within
   the remaining work budget; otherwise return the existing limit/error contract.

This is a proposed execution boundary, not an implemented GPU backend. Rank
compression is already useful without GPU, so it does not leave unused shader
plumbing in production.

## Evidence required before selecting GPU

Measure the same complete public result on the same machine. Include CPU
preparation, upload, dispatch, synchronization, readback, candidate storage,
exact intersections, topology, and output destruction. Report cold one-shot,
warm repeated, batched independent requests, and genuinely GPU-resident data
separately. Do not compare only a shader timer with a complete CPU call.

First measure the fraction `p` of current latency spent in candidate generation.
Even eliminating that stage entirely gives at most `1 / (1 - p)` end-to-end
speedup before transfer costs. If the remaining exact constructions or topology
dominate, improve those or reuse prepared work before investing in a GPU noder.

A proposed acceptance gate is at least 2x median end-to-end improvement on a
specified representative batch class, with repeated runs supporting the win and
without weakening exactness, memory limits, or tail latency. This is an engineering
gate, not a measured GPU result. An optional backend should stay opt-in until its
hardware/workload crossover is measured. Discrete-GPU and unified-memory results
need separate attribution, including adapter, driver, power state, and residency.
Software Vulkan and shader validation are useful correctness checks, not physical
GPU performance evidence. No physical GPU was exposed by the evaluation runner.

The first CPU comparison and its regressions are recorded in
[the paired measurement report](exact-noding-results.md). It is not GPU evidence.

## Verification and reproducibility

Unit tests compare complete candidate sets and every exact split parameter
against exhaustive pair enumeration. Cases include crossings, shared endpoints,
collinear overlaps, reversed edges, dense seeded arrangements, sparse long rows
on both axes, i64 extrema, subnormals, and maximal finite binary floating values.
The full existing reference matrices and fuzz replay remain release gates.

`benchmarks/exact-noding.rs` measures full Boolean intersection with analytically
known coordinate sequences. Comb polygons are tested in both orientations, with
32 through 2048 subject vertices, plus dense grids, a fractional intersection,
and a small rectangle control. Coordinates beyond the current specialization
range force the exact kernel; these are targeted synthetic workloads, not a GIS
or cross-library ranking. The direct-integer suite guards the shared index's
existing consumer. Both use the same paired-revision runner:

```sh
python3 scripts/benchmark-revisions.py --workload exact-noding \
  --base <merged-baseline> --head <candidate> --pairs 5
python3 scripts/benchmark-revisions.py --workload direct-certification \
  --base <merged-baseline> --head <candidate> --pairs 5
```

The JSON retains full result signatures, 21 calibrated batch samples per process,
execution order, CPU/compiler metadata, and source/lock hashes. Inspect regressions
and the rotated control as well as the intended win. Published timings must name
the measured commit; a design hypothesis is not a performance result.

## Alternatives and scope

Monotone-chain indexing is a credible next comparison: JTS documents combining
chain decomposition with a spatial index for noding. It may amortize bounds over
connected edge runs better than a per-edge tree. This implementation is independent
and does not copy JTS source. [MCIndexNoder documentation][jts]

A fully output-sensitive sweep may help adversarial boxes but requires an exact
batched-event policy for coincidences and overlaps. A grid can parallelize well
but must bound replication and seam work. Keep these as measured alternatives,
not an accumulation of unused engines. Open-line clipping, the other exact
short-circuit checks, face seeding, and offsets have additional bottlenecks and
are not made asymptotically fast by this closed-noder change alone.

This decision refines [the earlier exploration](next-generation-clipping-offsetting.md).
Neither CPU improvements nor GPU proposals establish a world-fastest claim.

[wgsl]: https://www.w3.org/TR/WGSL/#scalar-types
[jts]: https://locationtech.github.io/jts/javadoc/org/locationtech/jts/noding/MCIndexNoder.html
