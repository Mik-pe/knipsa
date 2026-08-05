# Beyond scanbeams: next-generation clipping and offsetting

This note is an engineering exploration, not a novelty or performance claim.
It separates published techniques from new combinations that still need to be
implemented, falsified, and benchmarked. A literature search cannot prove that
an idea is unpublished, and a design is not evidence that it beats Clipper2.

## What the current evidence actually says

The checked-in 2026-08-05 report is a historical result for commit `cc3c26c`.
A fresh run on 2026-08-05 against the pinned Clipper2 adapter initially matched
only 14 of 18 cases. The cause was not a novel topology problem: commit
`a6ca2df` had discarded the subject containment index whenever the subject was
a simple ring, even though clip-owned atomic edges still needed that index (and
vice versa). Restoring cross-operand classification and adding a regression
returned the live matrix to 18/18 matches.

Three independent post-fix processes matched all 18 signatures in every run.
The median-of-process-medians had Knipsa ahead on 8 of 18 tiny cases and a
Clipper2/Knipsa geometric-mean latency ratio of `0.85x` (so Clipper2 was about
`1.18x` faster overall on this machine/run). Clipper2 led on concave crossing,
self-crossing union, shared-edge operations, and many horizontal edges. These
results identify current profiling targets and supersede the speed conclusion,
but not the reproducibility metadata, of the older checked-in report.

Offsetting has no shared Knipsa/Clipper2 workload or native comparison in this
repository. Any claim that either implementation wins offsetting is currently
unsupported.

## Constraints worth preserving

- The exact rational arrangement is a semantic oracle, not the production hot
  path. It provides a useful independent failure detector.
- `i64`, binary `f64`, fill rules, coincidences, regularization, and output
  canonicalization are separate contracts. A faster event queue cannot repair
  an unspecified topology policy.
- Exact predicates and exact constructions are different costs. Most events
  only need a sign; rational coordinates should be materialized only when an
  output vertex or stable event identity needs them.
- Clipper2 is an external conformance and performance reference. No source is
  copied or translated.

## Published building blocks

### Output-sensitive intersection discovery

Bentley--Ottmann replaces all-pairs segment testing with an ordered event queue
and active structure. The classical bound depends on both edge count and the
reported intersections. This is the obvious asymptotic replacement for
`boolean.rs`'s all-pairs splitter, but equal points, overlapping segments, and
horizontal runs require a deterministic batched-event policy; the textbook
non-degenerate algorithm is not a ready polygon kernel.

Source: Bentley and Ottmann,
[*Algorithms for Reporting and Counting Geometric Intersections*](https://doi.org/10.1109/TC.1979.1675432).

### Adaptive exact predicates

Shewchuk's adaptive method evaluates only enough precision to certify a
determinant's sign. The important lesson for Knipsa is architectural: start
with a cheap error-bounded filter, escalate an uncertain sign, and reserve a
general exact representation for the hard minority. It does not, by itself,
provide exact intersection coordinates or polygon topology.

Source: Shewchuk,
[*Adaptive Precision Floating-Point Arithmetic and Fast Robust Geometric Predicates*](https://www.cs.cmu.edu/~quake/robust.html).

### Noding and topology as separate stages

JTS OverlayNG explicitly decouples segment noding from topology construction
and offers progressively more robust noding strategies. It also reduces the
input to the overlap envelope and has special handling for already-noded
coverages. This is strong evidence for a portfolio rather than one universal
kernel.

Source: [JTS OverlayNG package documentation](https://locationtech.github.io/jts/javadoc/org/locationtech/jts/operation/overlayng/package-summary.html).

### Offsets as Minkowski sums or wavefronts

CGAL implements both Minkowski-sum offsetting and straight-skeleton
wavefronts. Convolution is attractive for a one-shot outward Euclidean offset;
a straight skeleton is attractive for inward offsets and repeated distances
because the expensive skeleton can be reused. CGAL also distinguishes exact
predicates from exact constructions and can retain circular arcs rather than
flattening them immediately.

Sources:

- [CGAL 2D Minkowski Sums](https://doc.cgal.org/latest/Minkowski_sum_2/index.html)
- [CGAL 2D Straight Skeleton and Polygon Offsetting](https://doc.cgal.org/latest/Straight_skeleton_2/index.html)
- [Clipper2 offset arc tolerance](https://www.angusj.com/clipper2/Docs/Units/Clipper.Offset/Classes/ClipperOffset/Properties/ArcTolerance.htm)

## Proposed architecture: certified local overlay

This is a new combination for Knipsa, not a claim of a new algorithm in the
academic sense.

### 1. Dispatch by geometry and operation

Inspect once and attach certified properties to every input ring: bounds,
orientation, simplicity, convexity, orthogonality, monotonicity, coordinate
range, and whether its edges are already noded. Dispatch in this order:

1. empty/disjoint/containment identities;
2. rectangle and convex windows;
3. orthogonal overlay;
4. valid coverage union;
5. general local overlay;
6. exact arrangement fallback.

The certificate matters: a specialized result is accepted only when its
preconditions and output invariants are verified cheaply. This avoids the
current pattern of rediscovering simplicity in several helpers and makes a
fallback an expected tier, not an error.

### 2. Broad phase in deterministic tiles

Replace one global active list with a two-level structure:

- radix-sort edge bounding boxes into coarse integer tiles;
- test pairs only inside shared tiles, assigning every pair to one canonical
  tile so it is processed once;
- use a tiny sweep or brute-force kernel per tile depending on occupancy;
- send long edges to a separate interval index rather than duplicating them
  through many tiles.

This resembles a BVH/grid broad phase more than a classical scanbeam. It is
parallelizable, cache-friendly, and degrades explicitly: dense tiles can fall
back to a local sweep, while the exact oracle still handles uncertified cases.
The hypothesis is that real geometry is spatially sparse even when global
edge count is large. Adversarial all-crossing input remains quadratic in its
output and must be benchmarked as such.

### 3. Predicate ladder, construction on demand

For integer input:

1. use checked `i128` determinants where their magnitude is provably safe;
2. escalate overflow to `BigInt` only for the sign;
3. store an intersection initially as `(edge_a, edge_b)` plus certified order;
4. construct and reduce the rational coordinate only if it becomes an output
   vertex or is needed to break an event tie.

For `f64`, use an error-bounded determinant filter, then an expansion or the
existing exact binary-rational representation. A fixed decimal key may remain
an optimization key, but never the sole identity for topological equality.

### 4. Label components, not every atomic edge

The current fast arrangement samples both sides of each atomic edge and may do
point-in-polygon work repeatedly. Instead:

- construct local half-edge adjacency for the noded boundary;
- classify one seed face per connected component;
- propagate subject/clip winding across an edge using its signed operand
  contribution;
- emit only edges whose adjacent face labels differ under the requested
  Boolean operation.

This turns repeated spatial containment queries into graph propagation. The
exact oracle can compare every emitted boundary while the representation is
new. Shared and reversed edges need algebraic contribution merging before
propagation.

### 5. Stable event batches

All events at the same exact point form one batch. Within a batch, order by
event role, direction quadrant, exact orientation, operand, path, and edge ID.
Collinear overlaps become maximal one-dimensional intervals before active
ordering. This avoids deriving topology from insertion order or an epsilon and
makes parallel tile results deterministic at seams.

## Proposed offset portfolio

### One-shot outward offset

Generate the boundary convolution with a disk (or the requested polygonal join
kernel), index only overlapping convolution pieces, and run the certified local
overlay. Convex chains can be merged by angle in linear time. Reflex features
are the only parts likely to require topology cleanup.

### Inward or many-distance offset

Build a straight-skeleton-like wavefront once and query it at distance `d`.
Use a priority queue of collapse/split events with filtered exact predicates.
For a single tiny polygon this setup loses; for many distances, large inward
offsets, or animation it can amortize well and discovers topology changes
directly instead of generating self-overlap and unioning it afterward.

### Arc-native intermediate representation

Keep round joins as `(center, radius, start_angle, sweep)` until the final API
boundary. Intersect segment/arc and arc/arc pieces with certified predicates,
then flatten once using a declared Hausdorff bound. This avoids creating many
short segments only to feed them through Boolean cleanup. A future arc-aware
API could return exact conic pieces; the existing path API would still flatten.

The current `MAX_ARC_STEPS` cap can silently make a requested very small arc
tolerance unattainable. The API should either return an explicit tolerance
error, report the achieved bound, or document the cap as a best-effort limit.

### Local feature-size tolerance

A single global arc tolerance oversamples isolated gentle corners and may
undersample near a nearby edge. Use the requested tolerance as a hard maximum,
then tighten only where a spatial query finds nearby non-incident features.
This spends vertices where a topological collision is plausible. It needs a
proof that local refinement never exceeds the public error bound.

## Concrete code findings and priorities

| Priority | Finding | Proposed action | Proof needed |
| --- | --- | --- | --- |
| P0 | Cross-operand containment indexes were incorrectly elided, causing 4/18 benchmark signature mismatches. | Keep the index whenever edges of the other operand exist; retain the focused regression. | 18/18 matrix, full tests, fuzz seeds. |
| P0 | There is no offset conformance/performance matrix. | Add shared closed/open, join/cap, hole, collapse, large-coordinate, and tolerance workloads to Knipsa and Clipper2 adapters. | Canonical region/error comparison and three-process timings. |
| P1 | Exact Boolean intersection discovery is all-pairs. | Add an exact event-discovery interface; feed its splits into the unchanged classifier/tracer first. | Differential equality against all-pairs oracle. |
| P1 | `offset_paths64` used absolute `f64` conversion and rejected small shapes beyond 2^53. | Translate to a shared local origin, compute checked differences in `i128`, restore with checked `i128`. | Translation metamorphic tests and overflow edges. |
| P1 | Fast-path classification and short-circuit helpers can rescan rings. | Build one per-request property cache and pass it through dispatch. | Benchmark without changing selected result. |
| P1 | Fixed 32-row containment buckets ignored input size/distribution. | The first implementation now selects a power-of-two count from edge count, capped at 64; y-distribution and inline storage remain future work. | Worst-case memory cap and profile evidence. |
| P2 | `KEY_SCALE` is both an acceleration key and an implicit topology identity. | Separate approximate lookup keys from exact vertex identity; collisions must fall back or be disambiguated. | Adversarial points below the quantization spacing. |
| P2 | Offset predicates use one absolute epsilon across scales. | Use scale-aware filters with exact/extended fallback for sign decisions. | Scale and translation metamorphic tests. |
| P2 | Outline finiteness checking cloned the entire generated path. | Validate and return the owned vector. | Existing unit/coverage gates. |

The first adaptive-bucket A/B experiment used ten interleaved process pairs
against commit `434545a`. Median paired speedups were `1.52x` for
self-crossing EvenOdd union, `1.31x` for concave crossing, and `1.23x` for the
many-horizontal case; all 18 signatures still matched Clipper2. Unaffected
sub-microsecond cases remained noisy, so this is evidence for those targeted
paths rather than a blanket speed claim.

## Experiments that can kill the ideas quickly

1. **Tile overlay:** uniform sparse, long-edge grid, one dense tile, all-crossing,
   horizontal stacks, and repeated overlaps. Measure candidates, exact
   escalations, allocations, and seam merges in addition to latency.
2. **Face propagation:** compare number of point-in-polygon edge queries with
   face seeds on the existing workload and randomized arrangements.
3. **Lazy constructions:** count predicate calls, `BigInt` signs, rational
   materializations, and reduced `BigInt` coordinates. Reject the design if
   event bookkeeping costs more than eager construction on ordinary inputs.
4. **Offset portfolio:** one distance versus 10/100 distances on convex,
   reflex-heavy, holes, narrow corridors, and collapse-heavy shapes. Include
   achieved Hausdorff error and output vertices.
5. **Portfolio dispatch:** log which certified path ran. Every specialized
   result must be compared with the exact oracle in tests; no silent fallback
   or skipped reference case is a pass.

## Recommended implementation order

1. Land the containment regression and integer-offset recentering independently.
2. Add offset reference workloads before optimizing offset construction.
3. Introduce a reusable property cache and remove duplicate ring scans.
4. Prototype adaptive tiled broad phase behind the existing split interface.
5. Add face-label propagation while retaining current midpoint classification
   as the differential oracle.
6. Prototype arc-native and wavefront offsetting only after the offset matrix
   exposes which workload family justifies their complexity.
