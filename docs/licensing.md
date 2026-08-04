# Licensing and reference-code policy

This project is released under MIT OR Apache-2.0. It is an independent
implementation and currently contains no Clipper2 source code.

The inspected Clipper2 reference is distributed under the Boost Software
License 1.0. No Clipper2 source code is included in this repository.

## Rules for future work

- A clean-room Rust implementation may use Clipper2 documentation, published
  behavior, and locally built reference binaries for testing.
- If any Clipper2 source, test, fixture, or derived source is copied or
  translated into this repository, retain the original copyright notice and
  the complete Boost license text next to the affected material, and record
  its source revision in [`clipper-analysis.md`](clipper-analysis.md).
- Do not mix a copied/translated file into the MIT/Apache-owned Rust core
  without an explicit per-file licensing notice and review.
- Do not add code from a source whose license is missing, incompatible, or
  unclear. Replace the algorithm/reference instead.
- Keep third-party code isolated where practical, with a short provenance note
  and build instructions.

This is an engineering policy, not legal advice. Before distributing a port
that contains translated reference code, obtain a proper license review.
