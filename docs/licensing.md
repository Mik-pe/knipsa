# Licensing and reference-code policy

This project is released under MIT OR Apache-2.0. It is an independent
implementation and currently contains no third-party implementation source.

The reference matrix uses external builds of libraries with their own licenses.
Their output may be used as test data; their source and upstream fixtures are
not automatically part of this repository.

The checked-in adapter commands currently fetch or use, outside the Rust
crates, Clipper2 at the pinned revision under the Boost Software License 1.0,
Boost 1.91.0 under the Boost Software License 1.0,
`earcutr` 0.5.0 under ISC, `martinez-polygon-clipping` 0.8.1 under MIT, and
`geo` 0.33.1 plus its locked adapter dependencies under their recorded
Cargo-package licenses, JTS Core 1.20.0 under EPL-2.0 or EDL-1.0, and Shapely
2.0.7 under BSD-3-Clause (the wheel bundles GEOS 3.11.4 under LGPL-2.1). These
are test, triangulation, and benchmark tooling dependencies; downloaded
Clipper2, Boost, and JTS artifacts stay in the ignored `target/reference` tree
and are not linked into the Rust library. The Rust API uses `earcutr` as a
declared dependency, so its ISC license is part of the dependency audit but no
source is copied into this repository. When the `geo-types` feature is enabled,
`geo-types` is also linked under MIT OR Apache-2.0; it remains absent from the
default feature set. The optional Serde integration likewise uses Serde under
MIT OR Apache-2.0 and remains absent from the default feature set.

## Rules for future work

- A clean-room Rust implementation may use public behavior, documentation, and
  locally built reference binaries for testing.
- If any reference source, test, fixture, or derived source is copied or
  translated into this repository, retain its copyright notice and complete
  license text next to the affected material, and record its source revision
  in [`reference-matrix.md`](reference-matrix.md).
- Do not mix a copied/translated file into the MIT/Apache-owned Rust core
  without an explicit per-file licensing notice and review.
- Treat fixture licenses separately from implementation licenses. A test file
  is not automatically covered by the license of the library that loads it.
- Do not add code from a source whose license is missing, incompatible, or
  unclear. Replace the algorithm/reference instead.
- Keep third-party code isolated where practical, with a short provenance note
  and build instructions.

This is an engineering policy, not legal advice. Before distributing a port
that contains translated reference code, obtain a proper license review.
