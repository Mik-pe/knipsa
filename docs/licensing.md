# Licensing and reference-code policy

This project is released under MIT OR Apache-2.0. It is an independent
implementation and currently contains no third-party implementation source.

The reference matrix uses external builds of libraries with their own licenses.
Their output may be used as test data; their source and upstream fixtures are
not automatically part of this repository.

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
