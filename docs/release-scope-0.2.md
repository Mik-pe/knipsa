# Release scope for 0.2

Version 0.2 establishes the first production-oriented Knipsa release line. Its
primary contract is correctness, reproducibility, a coherent public API, and a
package that can be consumed independently from the repository checkout.

## What 0.2.0 guarantees

- `knipsa` and `knipsa-ffi` use the same semantic version and are released from
  the same Git commit and tag.
- Patch releases preserve the public Rust API and the 0.2 C ABI.
- Integer and floating-point Boolean operations, polygon and open-polyline
  offsets, point queries, rectangle clipping, simplification, and
  triangulation remain covered by the repository's deterministic tests.
- Certified fast paths fail closed to the general or exact implementation when
  their preconditions are not proven.
- Every checked-in external reference matrix must match semantically before a
  release. Performance measurements never replace correctness checks.
- The packaged core crate is built by Cargo and exercised from a separate
  consumer before publication.

## Patch-release policy

Version 0.2.1 and later 0.2.x releases may improve dispatch, allocation,
triangulation, or other internal performance without waiting for a new minor
release. Such changes must preserve observable 0.2 behavior, pass the complete
release matrix, and retain conservative fallbacks.

A benchmark improvement is reported only for the pinned workload, reference
revision, machine, and toolchain that were measured. A regression in one case
must not be hidden by a faster aggregate.

At the 0.2.0 release checkpoint, Knipsa matched all 12 checked-in Clipper2
triangulation cases. Very small cases have shown normal microbenchmark variance
between calibration runs, so their timing is evidence to reproduce rather than
a release guarantee. Further verified improvements are candidates for 0.2.1,
not correctness blockers for 0.2.0.

## Deliberately deferred work

The following additions need their own API and evidence work and are not part
of the 0.2.0 contract:

- Boolean clipping of open paths;
- explicit polygon-tree output with parent and hole relationships;
- caller-configurable floating-point scale or tolerance policies;
- located execution/topology errors beyond the new structured collection
  validation diagnostics;
- first-party WKT or similar text-format adapters beyond the new optional
  `geo-types` and Serde integrations;
- broader real-world GIS and CAM corpora across multiple architectures.

These omissions are documented so applications can evaluate Knipsa without
assuming capabilities that the current API does not promise. Compatible
additions may land in 0.2.x; changes to existing contracts require a later
pre-1.0 minor release.

The Rust API now provides opt-in `TriangulationLimits` for untrusted
triangulation requests. Boolean and offset operations still need a unified
public request-budget API before resource limits are complete across the whole
crate.

## Evidence boundary

The repository currently gates floating-point Boolean behavior against six
pinned implementations and has separate integer, offset, and triangulation
matrices against Clipper2. These finite matrices provide reproducible evidence
for their checked-in cases. They are not a claim of universal equivalence or
universal performance leadership.
