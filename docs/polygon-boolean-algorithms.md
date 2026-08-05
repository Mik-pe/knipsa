# Polygon Boolean Algorithms: research and kernel direction

**Scope.** This is a design and evaluation record for Knipsa's planar,
piecewise-linear polygon Boolean kernel. It is not a promise of compatibility
with a particular library and it does not import any third-party implementation.
The target is the four regular set operations (intersection, union, difference,
and XOR) on multiple closed paths, with explicit fill rules and a C ABI.

## Decision in one page

Keep the current **exact planar-arrangement** implementation as the semantic
oracle and test vehicle. Its all-pairs segment splitting is deliberately easy
to audit, but it is quadratic before output construction and arbitrary-precision
rationals make it the wrong long-term hot path. The production direction should
be a **Vatti-family scanbeam/sweep implementation with exact integer
predicates and a separately specified overlap policy**. It best matches the
existing `i64` API, all four fill rules, holes/multipolygons, the desired C
bridge, and the practical evidence supplied by Clipper2.

Do not replace the oracle with a direct port of Clipper2, and do not make
Greiner--Hormann, Weiler--Atherton, or Sutherland--Hodgman the general kernel.
They are valuable restricted fast paths and test references, but their control
flow does not make the difficult coincident-edge and self-intersection contract
easier to prove. A CGAL-style arrangement is the strongest general correctness
model and remains the right differential referee; its exact-construction and
DCEL costs are not the first choice for Knipsa's C-facing, integer-first fast
path.

This recommendation is an engineering judgement. The repository now has a
reproducible workload and adapter benchmark; its f64 fast path is measured
separately from the exact fallback, and every timing claim remains
machine-specific.

## Repository fit observed on 2026-08-05

The current working tree already goes beyond the README's older "next major
piece" wording:

- `BooleanRequest`/`BooleanRequestD` expose all four operations and `EvenOdd`,
  `NonZero`, `Positive`, and `Negative` fill rules.
- `src/boolean.rs` converts inputs to reduced arbitrary-precision rationals,
  splits crossings and collinear overlaps, classifies the two sides of atomic
  edges, and stitches directed boundary edges. The integer API explicitly
  returns `NonIntegralResult`; the `f64` API treats input binary floats as exact
  values during construction.
- Existing unit tests cover ordinary Boolean operations, holes, empty/disjoint/
  touching and identical inputs, a fractional intersection, and a fill-rule
  distinction. The C bridge has explicit result ownership and status mapping.
- `docs/reference-matrix.md`, `docs/testing-strategy.md`, and `benchmarks/`
  already establish the right policy: semantic-region comparison, pinned
  independent references, and kernel-only timing separated from FFI.
- `src/fast.rs` now adds a separate strict-convex two-ring path: a linear
  boundary walk produces split parameters and containment hints, while
  degenerate or uncertain input stays on the exact/general fallback.

The exact fallback splitter checks every edge pair, so its
intersection-discovery stage is **O(E^2)** before rational-cost and output
work. That is a concrete optimization boundary, not a reason to weaken the
exact semantics. Also note that arbitrary-precision exact coordinates prevent
floating predicate errors, but do not alone prove a correct topology policy:
zero-area, shared-boundary, and output-ring choices still require a contract
and tests.

## Terms that must be fixed before choosing an algorithm

1. **Input model.** A path is a closed sequence; a set of paths is interpreted
   under a named fill rule. Define whether self-intersecting paths are accepted
   for every rule, and whether input rings are merely contours rather than
   pre-labelled shells/holes.
2. **Regularization.** Decide whether zero-area results such as a shared edge
   are discarded. CGAL's Boolean-set package explicitly uses regularized
   operations (closure of the interior), a useful default for filled regions.
   Expose boundary/contact information separately if callers need it.
3. **Coincidence policy.** Specify exact treatment of shared vertices, partial
   and full collinear overlap, duplicate/reversed edges, horizontal edges, and
   repeated vertices. Never use an undocumented epsilon to decide topology.
4. **Numeric tiers.** `i64` input is exact but intersections can be rational;
   keep the current deliberate choice: integer output succeeds only when every
   output coordinate is integral. A later `Rational` output or scale-and-round
   API must be opt-in. `f64` output cannot preserve an arbitrary rational result
   after conversion, even if construction is exact.
5. **Canonical output.** Compare filled regions rather than ring start vertex
   or order; canonicalize orientation, rotation, component order, duplicates,
   and collinear policy at the API boundary. This is essential for FFI clients
   and reference testing.

## Algorithm families

| Family | Natural domain and strengths | Hard cases / robustness | Cost and implementation fit |
| --- | --- | --- | --- |
| Sutherland--Hodgman | Subject polygon clipped successively by a **convex** window; very small, streaming implementation. The original paper describes convex 2-D windows and re-enters the one-boundary routine. | Does not by itself provide general polygon-vs-polygon union/difference/XOR, holes, or coincident-edge semantics. Concave clipping requires decomposition or a different algorithm. | Roughly O(nm) for n subject vertices and m clip boundaries; excellent rectangle/convex fast path, poor general kernel fit. |
| Weiler--Atherton | Traversal between subject and clip boundaries; the original work targets concave polygons with holes. Useful historical model for output traversal. | Requires carefully ordered intersections and entry/exit transitions. Shared vertices/edges and tangencies make those transitions ambiguous unless a separate degeneracy rule is designed. It is not the easiest multi-contour/fill-rule engine. | Output-sensitive in friendly cases but pointer-rich traversal state. Keep as a study/test case, not Knipsa's primary kernel. |
| Vatti / scanbeam | General 2-D Boolean clipping: multiple arbitrary contours, holes, and fill rules; active edges, local minima, scanbeams, intersections, joins and output rings. | Robustness is in the full state machine: horizontals, equal scanline values, overlap joins, winding counts, and deterministic intersection ordering. Exact predicates/checked arithmetic are still mandatory. | Typical implementations sort events and maintain active edges; their practical performance is strong, but do **not** infer a simple universal Big-O from the paper or a library README. Best production fit after conformance hardening. |
| Greiner--Hormann | Elegant linked-list intersection/traversal algorithm for arbitrary closed polygons; the 1998 paper says it supports self-intersections and admits union/difference modifications. | Original GH does not properly handle degenerate intersections (common edges or a vertex intersection); the 2019 extension exists specifically to address this. Near-degeneracy remains numeric-model dependent. | Small and attractive for education or restricted inputs, but a degenerate-safe general version gains substantial case machinery. Not a shortcut to a production contract. |
| Martinez--Rueda--Feito | Sweep-line polygon overlay for all four Boolean operations; a good independent reference family and a useful event-ordering design study. | Event comparison, segment splitting, overlap, and floating comparison are its correctness centre. The JavaScript implementation is not an exact-integer oracle. Verify every claimed degeneracy behavior against its pinned version. | Sweep-line avoids naive all-pairs discovery in normal cases; suitable conceptual model for a Rust event queue/status structure, but not a source port. |
| Arrangement / sweep-line overlay | Split every intersecting/overlapping edge, classify faces or atomic edges, then extract a boundary. Handles holes and multipolygons naturally once the filled-set semantics are defined. | Exact construction plus a consistent DCEL/half-edge invariant offers the clearest audit story. Robust event ordering and overlap decomposition are still the hard part. | A naive arrangement is quadratic; a Bentley--Ottmann-style event sweep is output-sensitive after sorting, commonly expressed as O((E + K) log E) for non-degenerate segment intersection, where K is reported intersections. Degenerate overlap handling adds representation work and must be tested separately. Excellent oracle; optimise discovery rather than change semantics. |

### Primary and high-quality sources

- Sutherland and Hodgman, [*Reentrant Polygon Clipping* (1974)](https://dl.acm.org/doi/10.1145/360767.360802): convex-window, re-entrant clipping.
- O'Rourke et al., [*A Linear-Time Algorithm for Intersecting Convex Polygons* (1982)](https://www.cs.jhu.edu/~misha/Spring20/ORourke82.pdf): edge-walk rules used as the restricted convex-path design reference.
- Weiler and Atherton, [*Hidden Surface Removal Using Polygon Area Sorting* (1977)](https://www.cs.drexel.edu/~deb39/Classes/CS430/HWs/p214-weiler.pdf): its clipper handles concave polygons with holes.
- Vatti, [*A Generic Solution to Polygon Clipping* (1992)](https://doi.org/10.1145/129902.129906): scanbeam/general clipping formulation.
- Greiner and Hormann, [*Efficient Clipping of Arbitrary Polygons* (1998)](https://doi.org/10.1145/274363.274364), plus [Hormann, Agathos and Elber's degeneracy extension (2019)](https://doi.org/10.1016/j.cagx.2019.100007).
- Martínez, Rueda and Feito, [*A new algorithm for computing Boolean operations on polygons* (2009)](https://doi.org/10.1016/j.advengsoft.2008.10.005), and the maintained [Martinez implementation](https://github.com/mfogel/polygon-clipping) used only as a differential adapter.
- CGAL's [2-D regularized Boolean-set operations manual](https://doc.cgal.org/latest/Boolean_set_operations_2/index.html): exact-kernel example, polygons with holes, regularization, and arrangement/DCEL representation.

## Practical references: Clipper2 and CGAL

### Clipper2

[Clipper2's official overview](https://angusj.com/clipper2/Docs/Overview.htm)
documents integer and double path types, closed and open path clipping,
`EvenOdd`/`NonZero`/`Positive`/`Negative` fill rules, and intersection, union,
difference, and XOR. Its [robustness note](https://angusj.com/clipper2/Docs/Robustness.htm)
states that integer `Clipper64` is the most accurate class and that accuracy
gradually degrades as coordinates approach roughly +/-1e15. The project
documentation and Knipsa's existing `clipper-analysis.md` identify its Boolean
engine as Vatti/scanbeam family.

What to learn, rather than copy:

- the production value of an integer-first API, explicit fill rules, robust
  handling of overlaps/touches, and an extensive regression corpus;
- scanbeam invariants: local minima, active-edge ordering, horizontal handling,
  winding contribution, intersections, joins/splits, and ring ownership;
- deterministic normalization as an interoperability feature.

Clipper2 is a benchmark and semantic reference, not a correctness proof or an
appropriate runtime dependency. Its stated integer range/precision trade-off
also differs from Knipsa's current arbitrary-precision construction. Do not
claim Clipper compatibility without case-by-case conformance evidence.

### CGAL

CGAL provides **regularized** Boolean set operations on polygons and polygons
with holes, with an `Arrangement_2`/DCEL representation. Its manual recommends
an exact-predicates/exact-constructions kernel in the examples and explains
that Boolean results can contain holes and disconnected components. It also
supports general polygons bounded by x-monotone curves, beyond Knipsa's current
segment-only scope.

CGAL is the high-assurance design reference and optional exact referee. It is
not the recommended implementation substrate for Knipsa: its C++ templates,
kernel/traits choices, arbitrary number types, and object model would create a
large C++ bridge and make Knipsa's small stable C ABI dependent on a foreign
ABI/lifetime system. An independent external adapter is preferable.

## Capability and risk comparison for Knipsa

| Requirement | SH | WA | Vatti/Clipper2 style | GH | Martinez sweep | exact arrangement / CGAL style |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| General concave operands | no | yes | yes | yes | yes | yes |
| Holes and multipolygons | no | yes, complex | yes | claimed, complex | yes | yes |
| Self-intersecting contours / named fill rules | no | not a natural model | strong fit | claimed but degeneracies problematic | model/version dependent | strongest when semantics are defined |
| Shared edges, touching, tangency | special case | special case | core state-machine work | original algorithm insufficient | core event-policy work | core splitting/classification work |
| Exact rational construction | easy only in narrow domain | possible, not natural | possible but expensive | possible, not natural | possible but event ordering becomes hard | natural |
| Fast production path | only restricted case | no clear advantage | recommended | not recommended | candidate design | after sweep optimisation |
| Idiomatic Rust + C bridge | excellent restricted helper | awkward linked topology | good: owned Vec state, POD boundary | awkward mutation-heavy lists | good conceptual influence | good safe internal model; DCEL must stay private |

The table records architectural fit, not a claim that any unchecked algorithm
automatically accepts malformed or self-crossing input. In particular,
``near-degenerate'' is not an input class: for exact integers it becomes a
precise topological relation; for floats it requires a documented conversion,
scaling, or tolerance policy.

## Staged implementation recommendation

### Stage 0 — make the contract executable (before optimisation)

1. Add corpus cases for each operation × fill rule × input profile, including
   shared endpoint, T-junction, touch at vertex, identical/reversed edge,
   partial/full collinear overlap, horizontal edges at equal scanbeam levels,
   zero-area/repeated/collinear paths, nested holes/islands, bow ties, and
   very large `i64` coordinates.
2. Specify regularized semantics and whether self-intersecting contours are
   valid for each fill rule. Make empty/error outcomes part of the corpus.
3. Canonicalize results and compare filled region plus topology, never raw
   order. Preserve non-integral integer results as a deterministic error.
4. Differential-test all applicable profiles against the pinned Clipper2,
   Martinez, GEOS/JTS, Boost.Geometry, and optional CGAL adapters already
   anticipated by `docs/reference-matrix.md`. Record disagreements rather than
   majority-voting them away.
5. Fuzz at the Rust and C boundary with deterministic seed replay, asserting
   no panic, no undefined behaviour, valid rings, and Boolean algebra laws
   where the defined semantics permit them.

### Stage 1 — retain the arrangement oracle

Finish audit tests around the present rational splitter and ring tracer. Give
the internal model names/invariants: atomic edges do not cross in their
interiors; each retained directed edge separates result interior from exterior;
each traversal consumes an edge once; output orientation/ownership is
canonical. This baseline is slow by design but is valuable for minimizing
failures and checking the fast kernel.

### Stage 2 — accelerate only intersection discovery

Introduce a separate internal scanbeam/event-sweep module behind the same
oracle-tested interface. Start with exact `i64 -> i128` predicates and checked
comparisons; promote to rationals only when a construction requires it. Do not
use `f64` as a sweep-key tie-breaker. Define a total event order for equal
coordinates and explicitly decompose collinear overlap before active-edge
ordering. Feed the resulting atomic edges into the existing classifier/tracer
first; that isolates performance change from topology change.

Benchmark Stage 2 against the repository's versioned workloads, reporting
kernel-only and end-to-end results separately. Add adversarial cases where
K approaches E², where exact arithmetic grows, and where many segments share
one coordinate; asymptotic improvements must not hide catastrophic constants.

### Stage 3 — scanbeam output construction only after equivalence

If profiling shows atomic-edge materialization/tracing dominates, implement
Vatti-family output construction with its own exhaustive invariants and retain
the arrangement oracle under test/fuzz builds. Gate it on exact semantic
equivalence across the required corpus, metamorphic tests, and C ABI smoke
tests. This is where Clipper2 is most useful as an external performance and
behavior comparator, not as source to translate.

### Stage 4 — API and release hardening

Keep Rust allocation and all topology objects private. The C layer should
continue to expose only fixed-width point/path descriptors, operation/fill
enums, status values, and one explicit destructor. Add size/overflow limits,
allocation-failure behavior, and a versioned optional rational/scaled-output
API before promising full `i64` Boolean closure. Do not expose a C++ or CGAL
object through the ABI.

## Acceptance evidence

No algorithm milestone is "stable" or "fast" until it has all of:

- exact-head reproducible reference-matrix runs with pinned versions;
- minimized fixtures for every historical mismatch and crash;
- property tests: commutativity where applicable, idempotence, empty laws,
  XOR self-cancellation, and area/containment/topology agreement;
- targeted overlap/touching and high-coordinate tests, plus fuzz seed replay;
- C11 ABI tests covering null, empty, bad enum, ownership/free, and all error
  paths; and
- benchmark reports following `docs/benchmarking.md`, including CPU/toolchain,
  workload hashes, allocation behavior, medians and tail latency.

Correctness is the release gate; a faster kernel with a single unexplained
semantic mismatch is a regression, not an optimisation.
