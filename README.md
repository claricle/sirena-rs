# etassia-rs
Rust runtime for etassia with the Ruby native gem (etassia/etassia#343)

_Etassia_ comes from the Greek ἐκτάσσω, "drawn up in formation".

The bindings live in their own repositories: Ruby (`etassia-native`, this
repository's `bindings/ruby`), Python ([etassia/etassia-py](https://github.com/etassia/etassia-py))
and TypeScript ([etassia/etassia-ts](https://github.com/etassia/etassia-ts)).

## RELEASING

- **`etassia` (crates.io)**: release-plz opens the release PR from the
  conventional commits; merging it publishes the crate.
- **`etassia-native` (RubyGems)**: Actions > release-bindings > Run workflow;
  the gem pipeline cuts the bump PR, tags `etassia-native-vX.Y.Z` and publishes
  the six platform gems plus the source gem.

## DEVELOPMENT

`.github/workflows/ci.yml` generates the goldens with Ruby on each runner OS
(the corpus follows `Math.*` semantics, so Rust matches Ruby on the same
machine) and runs `cargo test --workspace` against them.
