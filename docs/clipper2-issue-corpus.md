# Clipper2 issue-derived robustness corpus

This corpus turns confirmed historical Clipper2 failure classes into
independent Knipsa regression data. It is an adversarial robustness suite, not
a claim that Knipsa reproduces Clipper2's implementation or output ordering.

The review snapshot is 2026-08-09. An issue is accepted only when the Clipper2
maintainer confirmed the problem, supplied a concrete fix, or explicitly said
it was fixed. A closed issue alone is not evidence. The executable cases live
in [`tests/fixtures/clipper2-issue-corpus-v1.json`](../tests/fixtures/clipper2-issue-corpus-v1.json)
and run in the ordinary Rust test suite.

## Accepted failure classes

| Issue | Upstream evidence | Independent Knipsa challenge |
| --- | --- | --- |
| [#381](https://github.com/AngusJohnson/Clipper2/issues/381) | Maintainer verified the union bug and announced a fix. | Mirrored wedges sharing a terminal vertex must union without direction-dependent failure. |
| [#408](https://github.com/AngusJohnson/Clipper2/issues/408) | Maintainer called it a bug and supplied offset changes. | Both orientations of a 180-degree open reversal must produce valid round strokes. |
| [#542](https://github.com/AngusJohnson/Clipper2/issues/542) | Maintainer verified the rectangle-clipping bug. | An oblique floating-point triangle must stay strictly inside the requested rectangle. |
| [#703](https://github.com/AngusJohnson/Clipper2/issues/703) | Maintainer reproduced several offset problems and reported them fixed. | A near-collinear artifact beside a large shell must not erase the shell during contraction. This is invariant-only because current Clipper2 retains an extra thin contour while Knipsa discards it. |
| [#715](https://github.com/AngusJohnson/Clipper2/issues/715) | Maintainer supplied and refined a hole-aware shrinking fix. | Contracting a shell must retain and expand its oppositely wound hole. |
| [#771](https://github.com/AngusJohnson/Clipper2/issues/771) | Maintainer supplied the exact open-path fix. | An out-and-back path crossing a box must retain its clipped open segment. |
| [#916](https://github.com/AngusJohnson/Clipper2/issues/916) | Maintainer confirmed and fixed excessive-deflation overflow. | Over-deflating a regular polygon must terminate with an empty result, not artifacts. |
| [#942](https://github.com/AngusJohnson/Clipper2/issues/942) | Maintainer posted a recursive ownership fix. | A framed hole containing an island must preserve alternating outer/hole ownership. |
| [#957](https://github.com/AngusJohnson/Clipper2/issues/957) | Maintainer agreed that the PolyTree hierarchy was wrong. | A frame assembled from touching rectangles must retain its hole and contained island. |
| [#975](https://github.com/AngusJohnson/Clipper2/issues/975) | Maintainer confirmed the infinite loop and reduced the reproducer. | Near-coincident sliver intersection must terminate and return valid geometry. |
| [#1056](https://github.com/AngusJohnson/Clipper2/issues/1056) | Maintainer diagnosed the endpoint test and posted corrected code. | A near-collinear quadrilateral at a huge origin must triangulate into positive triangles. |
| [#1067](https://github.com/AngusJohnson/Clipper2/issues/1067) | Maintainer confirmed the sliver-union bug and its faulty self-intersection path. | Positive-area slivers touching a triangle fan must not create invalid rings or spurious failure. |

Several reports are real but are not yet in the executable corpus. In
particular [#1029](https://github.com/AngusJohnson/Clipper2/issues/1029) has an
AddressSanitizer-confirmed PolyTree use-after-free, while #979 and #987 are
closely related infinite-loop reports. They need independently minimized
geometry before they can be added without importing reporter-owned fixtures.

## Rejected reports

| Issue | Why it is not a bug regression |
| --- | --- |
| [#1038](https://github.com/AngusJohnson/Clipper2/issues/1038) | The reporter confirmed that their code assumed an ordering not promised by `PolyTreeToPathsD`. |
| [#1008](https://github.com/AngusJohnson/Clipper2/issues/1008) | The maintainer judged both non-canonical outputs to describe the same filled region. |
| [#970](https://github.com/AngusJohnson/Clipper2/issues/970) | Touching output polygons are explicitly documented and may require a follow-up union. |
| [#923](https://github.com/AngusJohnson/Clipper2/issues/923) | The observed tiny-result removal follows documented integer rounding behavior. |
| [#934](https://github.com/AngusJohnson/Clipper2/issues/934) | Extremely short input edges require caller-side simplification before offsetting. |
| [#921](https://github.com/AngusJohnson/Clipper2/issues/921) | No sample data was supplied, so the report cannot be reproduced. |
| [#425](https://github.com/AngusJohnson/Clipper2/issues/425) | The maintainer confirmed that the one-unit difference was within `Clipper64` precision. |

## Fixture policy

The public issue is research provenance, not a fixture license. Corpus
coordinates are therefore synthesized locally from the reported failure class.
They deliberately change scale, origin, shape, or arrangement and encode a
semantic invariant instead of an upstream output sequence. Raw attachments,
test code, and reporter coordinate dumps are not committed.

Run `make conformance-clipper-issues` to extract the directly comparable cases
into temporary standard workloads and check them against the pinned current
Clipper2 reference. Cases marked `invariant_only` remain in Knipsa's regression
suite but are omitted from differential equality when the two libraries make a
documented semantic choice that is not itself the tested invariant.

When extending the corpus:

1. Record an explicit maintainer confirmation or fix, not merely the issue's
   closed state.
2. Minimize or synthesize a new case and state the invariant it exercises.
3. Add an exclusion entry when investigation shows documented behavior,
   invalid input, insufficient evidence, or a caller error.
4. Keep the case in the normal test suite so a hang, panic, invalid ring, lost
   hole, or invented topology fails CI.
