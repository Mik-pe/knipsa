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
5. Run `./scripts/fuzz-replay.sh` after geometry-kernel or input-validation changes.

Prefer deletion and one canonical implementation. While Knipsa is pre-1.0, do
not add or retain aliases, wrappers, duplicate algorithms, fallback branches,
feature flags, or dead code solely for backward compatibility. Preserve a
compatibility path only for a demonstrated downstream contract or an explicit
owner decision; document its owner and removal condition. Code is a liability,
and version control is the archive (see Google SRE's
[Simplicity](https://sre.google/sre-book/simplicity/)).

Do not claim conformance or a speed win without a reproducible run of the
reference matrix and benchmark protocol. Keep reference versions pinned in
`docs/reference-matrix.md` when an adapter changes.

Read `docs/licensing.md` before copying or translating any third-party source.
When provenance or license terms are unclear, do not import the code.
