# knipsa

`knipsa` is a from-scratch, idiomatic Rust polygon clipping engine inspired by
the problem solved by Clipper2. The name is Swedish: *knipsa* means “to nip”
or “to snip”, like a crab does with a claw.

> **Status: test-first pre-kernel phase.**
>
> The boolean scanbeam engine is intentionally not claimed as implemented yet.
> The current public tree establishes the geometry contracts, error model,
> language-neutral FFI boundary, coverage gate, and conformance plan that the
> engine must satisfy before it is released.

The algorithm and APIs are independent. knipsa is not API- or ABI-compatible
with Clipper, and it does not copy Clipper's C++ DLL layout. Compatibility will
be measured semantically through differential tests, while the Rust and FFI
APIs are designed for knipsa's own invariants.

## Current foundation

- checked integer geometry primitives and deterministic path validation;
- explicit operation/fill-rule types matching the problem domain, without
  inheriting Clipper's calling convention;
- a small C-compatible FFI with borrowed flat slices, explicit status codes,
  and no C++ references or allocator sharing;
- Rust unit tests, C-header smoke tests, a pinned Clipper2 research record,
  and a 100% coverage gate for the code that exists today;
- a roadmap for corpus, property, differential, fuzz, ABI, and performance
  testing before a kernel can be called compatible or faster.

## Development

```text
cargo fmt --all -- --check
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
./scripts/check-c-api.sh
./scripts/coverage.sh
```

`cargo llvm-cov` is required by the coverage script. Install it with
`cargo install cargo-llvm-cov` or use the CI workflow.

## Design rules

1. A green coverage number is never treated as proof that polygon clipping is
   correct. Every kernel feature needs semantic tests and adversarial inputs.
2. The Clipper2 implementation is an oracle and research reference only. The
   pinned source revision and license notes live in
   [`docs/clipper-analysis.md`](docs/clipper-analysis.md).
3. No speed claim is accepted without reproducible benchmarks against a
   pinned reference build, with the same inputs, output checks, compiler class,
   and CPU details.
4. The FFI is a separate contract. It may evolve by versioned additions and
   must never expose Rust ownership, layout assumptions, or panics to callers.

See [`docs/testing-strategy.md`](docs/testing-strategy.md) for the definition
of done for the first real clipping kernel.
