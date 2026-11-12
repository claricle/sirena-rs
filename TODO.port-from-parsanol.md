# Port the release pipeline to the parsanol model (2026-11-12)

The owner ruled: sirena-rs releases must copy the parsanol approach
(`~/src/parsanol/parsanol-ruby/.github/workflows/release.yml` +
`parsanol-rs/release-plz.toml`), not hand-rolled platform gem builds or
local `gem push`.

## Why this is a port and not a workflow swap

parsanol-ruby is an **rb_sys extension gem**: `ext/parsanol_native`
(magnus) plus a workspace-member **vendored cdylib crate**
(`ext/parsanol_cdylib`, pure C ABI, `publish = false`). That extension
skeleton is what lets `oxidize-rb/actions/cross-gem@v1` build **all five
platform gems on one ubuntu runner** (it drives the per-target cargo
build through rake-compiler with `CARGO_BUILD_TARGET` set, and the
Rakefile's `vendor_cdylib` task copies the triple's cdylib into
`lib/parsanol/native/`). sirena-native is a plain **fiddle** gem with no
ext hook, so cross-gem would emit only a binary-free ruby gem today.

## Port steps

1. **Ext skeleton** `bindings/ruby/ext/sirena_cdylib/`:
   - `Cargo.toml`: workspace member, `name = "sirena_cdylib"`,
     `crate-type = ["cdylib"]`, depends on `sirena-ffi` (path), exposes
     the C ABI; `publish = false`.
   - `extconf.rb`: emit a Makefile whose `all` target runs
     `cargo build --release -p sirena_cdylib --target $CARGO_BUILD_TARGET`
     and copies `libsirena_cdylib.{so,dylib,dll}` into
     `lib/sirena/native/` (the dir `Native::PACKAGED_DIR` already reads);
     no-op Makefile for non-MRI engines (parsanol's pattern, issue #23).
   - Add the member to the root `Cargo.toml` workspace.
2. **gemspec**: list `ext/sirena_cdylib/extconf.rb` in `spec.extensions`,
   keep `SIRENA_GEM_PLATFORM` packaging only as the fallback, mark
   `required_ruby_version` and drop `scripts/build_platform_gem.rb`
   (superseded).
3. **`bindings/ruby/.github/workflows/release.yml`** modeled on
   parsanol-ruby's: dispatch bump_type / **auto-release on a version.rb
   bump landing on main** (the leptris-ruby#452 pattern); fixed
   `release` concurrency group; `fetch-ci-data` for supported platforms;
   `oxidize-rb/actions/cross-gem@v1` matrix; per-platform smoke jobs
   (install + render one SVG, exercising the fiddle load);
   `rubygems/configure-rubygems-credentials@main` (TP) pushing all gems
   with the repush-tolerant loop; GitHub release with generated notes.
4. **Rust crates**: copy `parsanol-rs/release-plz.toml` +
   `release-plz` workflow on push-to-main (CARGO_REGISTRY_TOKEN +
   the org PAT). Requires un-publishing `sirena-core` (currently
   `publish = false`) — **owner decision on the crates.io name** before
   this step. `sirena-wasm`/`sirena-py`/`sirena-ts` publish flows stay
   as-is until the ruby path is proven.
5. **Retire**: `platform-gems.yml` (5-OS matrix), the tag-triggered
   `release.yml` publish steps it replaces, and the TP setup note in
   README (bump-PR flow replaces tag dispatch).

## Release ops after the port

- Dispatch with `bump_type` (or merge a version bump) → bump PR →
  auto-merge → the merge IS the release: tag, cross-gem all platforms,
  smoke, TP-push, GitHub release.
- No local builds, no manual `gem push`, no per-OS machines.

## STATUS: implemented on branch release/parsanol-model (this file sits on the same branch)
Steps 1-3, 5-6 are done; step 4 (release-plz for the crates) still needs the owner decision on the crates.io name for sirena-core.
