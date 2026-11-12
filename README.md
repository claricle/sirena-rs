# etassia-rs
Rust runtime for etassia with Ruby/TS/Python bindings (etassia/etassia#343)

_Etassia_ comes from the Greek ἐκτάσσω, "drawn up in formation".

## RELEASING

Releases are cut by pushing a tag; `.github/workflows/release.yml` builds the
package, smokes it, and publishes it. The tag version must equal the manifest
version (nothing bumps it for you):

| Tag | Package | Registry | Manifest |
|---|---|---|---|
| `etassia-native-vX.Y.Z` | `etassia-native` (6 platform gems + source gem) | RubyGems | `bindings/ruby/etassia-native.gemspec` |
| `etassia-py-vX.Y.Z` | `etassia-py` (wheels + sdist) | PyPI | `bindings/python/pyproject.toml` |
| `etassia-ts-vX.Y.Z` | `etassia-ts` | npm | `bindings/ts/package.json` |

Dry run: Actions > release > Run workflow, pick a package, leave `publish` off.

One-time setup per registry (etassia/etassia-rs, workflow `release.yml`; also
create the GitHub environments `rubygems`, `pypi` and `npm`):

- **RubyGems** (owner of the `etassia-native` name): rubygems.org > Profile >
  Trusted publishers > add a pending publisher: gem `etassia-native`, owner
  `claricle`, repository `etassia-rs`, workflow `release.yml`, environment `rubygems`.
- **PyPI** (account that will own `etassia-py`): pypi.org > Publishing > add a
  pending publisher: project `etassia-py`, owner `etassia`, repository
  `etassia-rs`, workflow `release.yml`, environment `pypi`.
- **npm** (owner of `etassia-ts`): npm cannot trust a package that does not exist
  yet. Publish the first release once by hand (`npm publish --access public`
  from `bindings/ts` after `npm run build`), then npmjs.com > package >
  Settings > Trusted Publisher > GitHub Actions: org `etassia`, repository
  `etassia-rs`, workflow `release.yml`, environment `npm`. Later releases use the tag.
