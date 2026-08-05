# Reference and benchmark matrix

Knipsa is checked against several independent geometry implementations. The
references are test and benchmark tools only; none is a runtime dependency of
the Rust library.

## Reference set

| Reference | Role | Pinned adapter/version | License | Independence |
| --- | --- | --- | --- |
| [Clipper2](https://github.com/AngusJohnson/Clipper2) | Integer clipping, offsets, open paths | Native C++ adapter at pinned revision in `clipper-analysis.md` | Boost Software License 1.0 | Scanline/Vatti family |
| [GEOS](https://github.com/libgeos/geos) | OGC-style topology and robust overlay | Shapely 2.0.7 wheel, GEOS 3.11.4 | LGPL-2.1 | Same lineage as JTS |
| [JTS](https://github.com/locationtech/jts) | Large topology test corpus and Java oracle | Adapter pending; record version before use | EPL-2.0 or EDL-1.0 | Same lineage as GEOS |
| [Boost.Geometry](https://www.boost.org/library/latest/geometry/) | Generic polygon overlay and validity cases | Adapter pending; record Boost branch before use | Boost Software License 1.0 | Separate implementation |
| [Martinez](https://github.com/w8r/martinez) | Multipolygon boolean operations | `martinez-polygon-clipping` 0.8.1 | MIT | Separate sweep-line implementation |
| [CGAL](https://github.com/CGAL/cgal) | Optional exact-arithmetic referee | Adapter pending; package version and license review required | Package-specific; review before redistribution | Arrangement/set-operation implementation |

GEOS and JTS are both useful, but they are not counted as two independent
algorithm families. A case that passes both is stronger evidence about API
and port behavior than about algorithmic independence.

[Wagyu](https://github.com/mapbox/wagyu) is not part of the independent set:
its license states that parts of the library are derived from Clipper 6.4.0.
It can be evaluated later as a downstream compatibility target without
copying its source or treating it as a separate algorithm.

## Semantic profiles

Every case belongs to a profile. A reference may only be marked `not
applicable` when the profile explicitly allows it.

### `integer-closed-v1`

The first mandatory profile. It uses finite `i64` coordinates, valid closed
polygons, no implicit coordinate rounding, and the four boolean operations:
intersection, union, difference, and XOR. Results are compared as filled
regions, not as raw vertex order.

### `holes-and-multipolygons-v1`

Disjoint components, holes, nested rings, touching boundaries, and cases that
produce multiple output components. The profile records the fill rule and
requires topology-preserving canonicalization.

### `degenerate-input-v1`

Repeated vertices, collinear edges, zero-area rings, touching edges, and
self-intersections. These cases are only compared where all participating
references document compatible input semantics. Otherwise they become
reference-specific regression tests rather than forced cross-library matches.

### Feature-specific profiles

Open paths, offsets, floating-point coordinates, and exact-rational outputs are
separate profiles. A library that does not implement a feature cannot silently
turn a missing result into a pass.

## What 100% means

The conformance gate is green only when every case in every required profile:

1. runs to completion in every required adapter;
2. returns a valid result or the profile-declared error;
3. matches the canonical filled region and topology;
4. has no timeout, panic, undefined behavior, or unexplained reference
   disagreement.

A reference disagreement is recorded as a semantic split with the exact
versions, inputs, and outputs. We do not choose a majority result and call the
case passed.

## Provenance

Each case records:

- stable case ID and profile;
- operation, fill rule, and coordinate model;
- input hash and generated seed, when applicable;
- reference versions and adapter commands;
- fixture provenance and license, if an upstream case is imported.

The preferred corpus is generated or minimized in Knipsa's own format. Raw
upstream source and test files stay outside the production crate unless their
license and attribution are explicitly recorded.
