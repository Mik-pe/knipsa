# Knipsa contributor contract

Knipsa is a from-scratch Rust implementation of polygon geometry contracts.
Reference implementations are test and benchmark tools, not production
dependencies.

Before changing geometry code:

1. Add or update a deterministic regression test.
2. Run `cargo fmt --all -- --check`, `cargo test --workspace --all-features`,
   and `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
3. Run `./scripts/check-c-api.sh` for ABI changes.
4. Run `./scripts/coverage.sh` when coverage-sensitive code changes.

Do not claim conformance or a speed win without a reproducible run of the
reference matrix and benchmark protocol. Keep reference versions pinned in
`docs/reference-matrix.md` when an adapter changes.

Read `docs/licensing.md` before copying or translating any third-party source.
When provenance or license terms are unclear, do not import the code.
