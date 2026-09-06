# Execution design: exact topology, spatial candidates, conditional GPU offload

## Decision and implemented boundary

Keep one geometry kernel and one public precision contract. The measured next
step is CPU candidate selection, not an unverified GPU backend. The public API,
dependencies, feature flags, and complexity limits are unchanged.

`spatial.rs` contains the shared closed-box hierarchy. Integer ring certification
and general closed-path noding both use it. `boolean/noding.rs` prepares boxes and
streams original edge IDs to the existing rational splitter. Atomic-edge merging,
face winding, and ring tracing continue to own geometry. The old X-only active
sweep is removed rather than retained as another backend.

The old noder already sorted by X, but scanned every active edge even when Y
intervals were disjoint. Two-dimensional pruning removes that pathology. Dense
box overlap can still require quadratic traversal; this is not a new worst-case
output-sensitive clipping algorithm. [Measurements and regressions](exact-noding-results.md)
show the benefit and cost on specific synthetic workloads, not a world-fastest claim.

## Native exact bounds, otherwise exact ranks

First try the existing exact `Rational::to_i64` conversion for every bound. It
succeeds only for integral, representable values. When all succeed, those bounds
already provide the required exact order; no coordinate sorting is needed.
This is a representation optimization, not a different clipping algorithm.

If any endpoint is fractional or outside i64, rebuild **all** bounds into one
rank space. Both axis passes overwrite every component, including any already
converted prefix. Mixing native coordinates and ranks would be incorrect.

For each axis, sort rational endpoints by exact order. Equal coordinates receive
the same rank; distinct coordinates receive strictly increasing ranks. Therefore
`a < b` if and only if `rank(a) < rank(b)`, and all closed-box overlap tests are
unchanged. Shared points, zero-width boxes, tiny gaps, and enormous coordinates
are retained. Sort references, not cloned BigInts, and reuse the coordinate buffer
between axes. Exact ranking needs O(E log E) comparisons; native extraction is
linear. The shared index needs O(E log E) construction work and linear storage.
An exact comparison's cost depends on the operands' bit lengths.

Ranks are **not coordinates for constructing intersections**. A nonlinear monotone
mapping changes slopes. The splitter always uses original rational coordinates.
Ranks are also request-local, not persistent vertex identities: a future prepared
geometry or GPU cache must merge coordinate orders correctly or rebuild ranks,
and bind buffers to the request generation. Never reuse stale edge IDs or compare
ranks from separate preparations.

The index visits unordered pairs once and streams them rather than allocating
an O(K) candidate list. The integer certificate also avoids redundant arithmetic
at adjacent edges: a shared endpoint plus a checked nonzero cross product proves
there is no second intersection. Collinear and overflowing cases still defer;
non-adjacent contacts still require the full intersection contract.

## API and resource design

Keep the current convenience functions and `BooleanRequest`. Correct geometry
must not require users to pick a backend. Add an immutable prepared-geometry API
or reusable workspace only after repeated-query workloads justify preparation
cost and retained memory. Do not publish placeholder types or retain previous
requests' geometry unexpectedly.

The current preflight conservatively charges possible edge pairs. This change
does not make arbitrary million-edge sparse requests admissible under defaults.
Before relaxing that limit, charge visited nodes, exact predicates, arithmetic
growth, generated nodes, output size, and memory across the entire operation.
GPU batching does not remove denial-of-service and memory obligations.

## Concrete GPU experiment

WGSL's concrete numeric scalar types are i32, u32, f32, and f16; it has no built-in
arbitrary-precision rational type. Exact GPU geometry is not impossible, but a
shader translation of this kernel is a larger project. [WGSL scalar types][wgsl]

The first proposed offload is **box candidate generation only**:

1. Prepare exact ranks on the CPU and pack checked u32 bounds and edge IDs into
   an explicitly defined buffer layout. Validate cardinality and device limits.
   The current Rust i64/usize structs, including native signed bounds, are not
   a GPU ABI and must not be uploaded by byte reinterpretation.
2. Traverse disjoint tree tasks or tiles with closed interval comparisons and
   one deterministic owner per unordered pair. Bound long-edge replication.
   An E-by-E brute-force shader is not a replacement for spatial pruning.
3. Use bounded count/prefix-sum/write passes or bounded output chunks. Check
   counters and allocations. Device loss, overflow, or incomplete output must
   discard the candidate result, never silently truncate it.
4. Use the existing exact CPU splitter and topology on original coordinates.
   Any retry on CPU must fit within the remaining operation budget; otherwise
   return the existing limit/error contract.

This is a design, not an implemented GPU backend. The CPU representation and
shared index have real callers today; no unused shader plumbing is introduced.

## Hardware admission gate

First profile the candidate-generation fraction p of current end-to-end latency.
Even eliminating it entirely can yield at most `1 / (1 - p)` speedup before
transfer costs. If exact constructions or topology dominate, optimize those or
reuse prepared work first. This PR's full-call timings do not measure that fraction.

A proposed gate is a reproducible 2x median end-to-end improvement on a specified
representative batch class without weaker exactness, memory limits, or tail
latency. It is not a measured GPU result. Include preparation, upload, dispatch,
synchronization, readback, candidate storage, exact math, and result destruction.
Report cold one-shot, warm repeated, batched, and genuinely GPU-resident data
separately. Discrete and unified-memory GPUs need separate adapter, driver,
power-state, and residency attribution. Keep a backend opt-in until its crossover
is measured. Software Vulkan can validate correctness, not physical GPU speed.
No physical GPU was exposed in the inspected evaluation environment.

## Verification and alternatives

Tests compare complete candidate sets and exact split parameters against
exhaustive enumeration. They include contacts, overlaps, reversed edges, dense
seeded arrangements, sparse rows on both axes, native-prefix conversion failure,
i64 extrema, subnormals, and maximal finite input coordinates. Another exhaustive
test compares the adjacent-corner certificate with the full predicate.

The paired public-API workload covers both comb orientations with integral and
fractional input, dense grids, a constructed fractional vertex, and tiny controls.
The same runner serves the original integer certificate workload. Full output
signatures, raw samples, hashes, and regressions are retained. Exact-oracle tests,
reference matrices, fuzz replay, and the unchanged strict coverage remain gates.

Monotone-chain indexing is a credible next comparison; JTS documents combining
chains with a spatial index for noding. [MCIndexNoder][jts] A fully output-sensitive
sweep needs an exact batched-event policy for coincidences and overlaps; a tiled
GPU design must bound replication and seam work. Evaluate alternatives rather
than accumulating unused engines. Open-line clipping, exact short circuits, face
seeding, and offsets have other bottlenecks not solved by this closed-noder change.

This refines [the earlier exploration](next-generation-clipping-offsetting.md).

[wgsl]: https://www.w3.org/TR/WGSL/#scalar-types
[jts]: https://locationtech.github.io/jts/javadoc/org/locationtech/jts/noding/MCIndexNoder.html
