# Testing strategy

The goal is not to make the test counter large. The goal is to make a wrong
polygon result difficult to ship.

## Layers

### 1. Contract and unit tests

Every public geometry type, checked arithmetic path, validation error, enum,
and FFI status must have deterministic tests. These are the tests covered by
the initial 100% coverage gate.

### 2. Corpus tests

Import the Clipper polygon, line, offset, and polygon-tree fixtures into a
versioned knipsa corpus with attribution. Each case gets a stable identifier and
an expected semantic result, not merely a count or an area.

### 3. Algebraic property tests

For bounded integer coordinates and valid closed paths, test properties such
as:

- union and XOR commutativity;
- intersection and union idempotence;
- difference with an empty set;
- intersection with an empty set;
- XOR self-cancellation;
- output rings are closed by convention and contain no accidental duplicate
  vertices;
- area and containment agree with the returned topology.

### 4. Differential tests

Run the same serialized cases through a pinned Clipper2 reference executable
and knipsa. Compare canonicalized regions, not raw ring order. Any exception,
hang, overflow, non-finite result, or mismatch is a failing case saved as a
regression fixture.

### 5. Fuzzing and adversarial geometry

Fuzz malformed inputs, repeated points, horizontal edges, touching rings,
self-intersections, extreme coordinates, very small features, and operations
that create nested holes. Fuzzing must have a deterministic seed replay path.

### 6. FFI tests

Compile a C smoke client against the public header, exercise null/empty/valid
inputs, and verify exported symbols. Add language-level clients for at least
Python, Go, and one managed language before stabilizing the first release.

### 7. Benchmarks

Benchmark representative small, medium, pathological, and high-vertex cases.
Record compiler, optimization flags, CPU, coordinate type, operation, input
hash, output hash, and allocation behavior. A speed claim requires a report
that can be rerun from the repository.

## Release gate for the first clipping kernel

The first kernel is not release-ready until all of these are true:

- the deterministic corpus passes;
- differential tests pass against the pinned reference, with documented
  intentional semantic differences;
- property tests and fuzz replay cases pass;
- no panic, undefined behavior, or unbounded allocation is found on malformed
  FFI input;
- owned Rust code has 100% line, function, and branch coverage or an explicit
  reviewed exclusion;
- the FFI has a versioning policy and tested ownership rules;
- benchmarks are checked in and show the actual trade-offs.
