# Releasing knipsa

The `knipsa` and `knipsa-ffi` crates share one version and one Git tag. The core
crate must become available in the crates.io index before the dependent FFI
crate can be verified or published.

## Preconditions

1. Start from a clean commit on `main` with every required CI job green.
2. Update the workspace version, the `knipsa-ffi` dependency requirement, C
   header version macros, C smoke assertion, changelog, migration guide, and
   `release-tests/rust-consumer` version assertions.
   Keep [`release-scope-0.2.md`](release-scope-0.2.md) accurate when known
   limitations or patch-release policy changes.
3. Run `make release-check`. It packages the core crate, runs Cargo's publish
   dry-run, compiles a separate consumer against the unpacked core `.crate`
   artifact, and tests the FFI crate in the workspace.
4. Run the complete local gates documented in `AGENTS.md`, including C ABI,
   coverage, fuzz replay, and all pinned reference matrices.
5. Review `cargo package -p knipsa --list`. Cargo cannot package the dependent
   FFI crate until the matching core version exists in the registry; the
   publish workflow performs that second package verification between uploads.

## Publish order

Create and push the annotated tag only after the release commit is green:

```sh
release_version=$(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name == "knipsa") | .version')
git tag -a "v$release_version" -m "knipsa $release_version"
git push origin "v$release_version"
```

Dispatch the `Publish crates.io` workflow from that exact tag, enter the exact
workspace version,
and confirm `publish`. The protected `crates-io` environment must contain a
`CARGO_REGISTRY_TOKEN` secret and should require approval.

The workflow publishes `knipsa`, waits until crates.io reports the exact
version, performs a real FFI dry-run against the registry dependency, and then
publishes `knipsa-ffi`. Cargo does not permit overwriting a published version;
if the second publish fails, fix the release workflow or registry state without
re-tagging different source as the same version.

The same ordered release can be performed locally when Cargo is already logged
in:

```sh
cargo publish --locked -p knipsa
cargo info "knipsa@$release_version"
cargo publish --dry-run --locked -p knipsa-ffi
cargo publish --locked -p knipsa-ffi
```

Do not run the FFI commands until `cargo info` succeeds for the exact core
version.

## Verify

After publication, create a new empty project with no workspace patches, add
the exact released `knipsa` version, run the README example, and check the generated docs on
docs.rs. Repeat for `knipsa-ffi` and verify that its packaged header reports the
same version as `knipsa_version()`.
