# sirena-rs
Rust runtime for sirena with Ruby/TS/Python bindings (claricle/sirena#343)

## RELEASING

Releases are cut by pushing a tag; `.github/workflows/release.yml` builds the
package, smokes it, and publishes it. The tag version must equal the manifest
version (nothing bumps it for you):

| Tag | Package | Registry | Manifest |
|---|---|---|---|
| `sirena-native-vX.Y.Z` | `sirena-native` (6 platform gems + source gem) | RubyGems | `bindings/ruby/sirena-native.gemspec` |
| `sirena-py-vX.Y.Z` | `sirena-py` (wheels + sdist) | PyPI | `bindings/python/pyproject.toml` |
| `sirena-ts-vX.Y.Z` | `sirena-ts` | npm | `bindings/ts/package.json` |

Dry run: Actions > release > Run workflow, pick a package, leave `publish` off.

One-time setup per registry (claricle/sirena-rs, workflow `release.yml`; also
create the GitHub environments `rubygems`, `pypi` and `npm`):

- **RubyGems** (owner of the `sirena-native` name): rubygems.org > Profile >
  Trusted publishers > add a pending publisher: gem `sirena-native`, owner
  `claricle`, repository `sirena-rs`, workflow `release.yml`, environment `rubygems`.
- **PyPI** (account that will own `sirena-py`): pypi.org > Publishing > add a
  pending publisher: project `sirena-py`, owner `claricle`, repository
  `sirena-rs`, workflow `release.yml`, environment `pypi`.
- **npm** (owner of `sirena-ts`): npm cannot trust a package that does not exist
  yet. Publish the first release once by hand (`npm publish --access public`
  from `bindings/ts` after `npm run build`), then npmjs.com > package >
  Settings > Trusted Publisher > GitHub Actions: org `claricle`, repository
  `sirena-rs`, workflow `release.yml`, environment `npm`. Later releases use the tag.
