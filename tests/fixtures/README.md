# Conformance fixtures

This directory holds minimized, attributed regression cases. Fixtures must
record the profile, operation, fill rule, coordinate mode, input hash, and
semantic expectation.

Prefer Knipsa-generated cases in the canonical format. Do not add a raw
upstream file without recording its origin, revision, and fixture license in
the case metadata and [`docs/licensing.md`](../../docs/licensing.md).

[`clipper2-issue-corpus-v1.json`](clipper2-issue-corpus-v1.json) is the first
issue-derived robustness corpus. Its coordinates are original Knipsa fixtures,
not copied reports. [`docs/clipper2-issue-corpus.md`](../../docs/clipper2-issue-corpus.md)
records why each upstream report was accepted or rejected.
