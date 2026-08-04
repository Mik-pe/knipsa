# Knipsa contributor contract

Knipsa is a from-scratch Rust implementation of the polygon clipping
contracts exercised by Clipper2. The reference implementation is an oracle,
not a production dependency.

Before changing geometry code:

1. Add or update a deterministic regression test.
2. Run `cargo fmt --all -- --check`, `cargo test --workspace --all-features`,
   and `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
3. Run `./scripts/check-c-api.sh` for ABI changes.
4. Run `./scripts/coverage.sh` when coverage-sensitive code changes.

Do not claim Clipper compatibility or a speed win without a reproducible
differential test and benchmark report. Keep the reference commit pinned in
`docs/clipper-analysis.md` when the oracle changes.

Read `docs/licensing.md` before copying or translating any third-party source.
When provenance or license terms are unclear, do not import the code.
