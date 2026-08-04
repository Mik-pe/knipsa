# Clipper reference analysis

This document records the reference inspected before starting knipsa. It is an
analysis record, not a dependency declaration.

## Pinned source

- Repository: <https://github.com/AngusJohnson/Clipper2>
- Revision inspected: `f9c5eb6e14a59f6f5d65fbfb3564519a561cf4fd`
- Revision date in the checkout: 2026-04-20
- License: Boost Software License 1.0
- Local research checkout: a temporary clone, not part of knipsa

The repository reached through the historical `AngusJohnson/Clipper` URL had
the same current tree as `Clipper2` at the inspected revision. Clipper2 calls
the older generation “Clipper1” in its documentation and recommends Clipper2
for new work.

## Surface area

The C++ library exposes:

- boolean intersection, union, difference, and XOR;
- EvenOdd, NonZero, Positive, and Negative fill rules;
- closed and open paths;
- `int64` and scaled `double` coordinate paths;
- polygon offsetting with square, bevel, round, and miter joins;
- polygon/line rectangle clipping;
- Minkowski helpers and simplification/collinear trimming;
- nested polygon trees for holes and islands;
- optional Z-value callbacks;
- constrained Delaunay triangulation, which upstream currently warns is buggy.

The C++ core in the inspected checkout was approximately 9,857 lines across
the public headers and implementation files. The GoogleTest suite was
approximately 1,855 lines across 17 test translation units, backed by text
corpora for polygons, lines, offsets, and polygon-tree ownership.

## Algorithmic shape

The boolean engine is a scanbeam implementation derived from Vatti's general
polygon clipping algorithm. The important internal state includes:

- local minima and scanline queues;
- an active edge list and a sorted edge list;
- winding counts and fill-rule contribution decisions;
- horizontal edge processing;
- ordered edge intersections and swaps;
- output rings, joins, splits, ownership, and polygon-tree construction.

This is substantially more than a convex polygon intersection routine. The
first knipsa kernel milestone must therefore preserve the state-machine
invariants explicitly instead of hiding them behind a large untested port.

## Why knipsa has its own FFI

Clipper2's exported header represents paths as custom allocated arrays and
declares exported functions using C++ references, inline definitions, and
library-owned allocation. That is useful for its own supported bindings but is
not a portable ABI contract for knipsa.

knipsa's boundary is deliberately different:

- only fixed-width integer fields, pointers, lengths, enums, and status codes;
- no C++ references, templates, exceptions, Rust layout, or shared allocators;
- borrowed inputs and explicit ownership rules;
- versioned additions rather than accidental ABI promises.

The current FFI exposes only the implemented foundation operations. Boolean
and offset entry points will be added after their Rust semantics are locked.

## Compatibility position

knipsa will not promise byte-for-byte output ordering or the same error behavior
as Clipper. The semantic conformance contract will define equivalence for:

1. filled regions and holes;
2. open-path results where supported;
3. orientation and collinear policy;
4. coordinate precision and overflow behavior;
5. deterministic output canonicalization.

The reference remains pinned in this file whenever the oracle changes.
