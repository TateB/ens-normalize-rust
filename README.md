# ens-normalize-rs

Rust implementation of ENS normalization, ported from
[`@adraffy/ens-normalize`](https://github.com/adraffy/ens-normalize.js).
The crates.io package is named `ens-normalize`, which imports as
`ens_normalize`.

The crate embeds the same upstream derived data (`spec.json` and `nf.json`) and
tests against the upstream validation corpus from `@adraffy/ens-normalize`
`1.11.1` (Unicode `17.0.0`, CLDR `47`).

```rust
use ens_normalize::ens_normalize;

let name = ens_normalize("RaFfY.eth")?;
assert_eq!(name, "raffy.eth");
# Ok::<(), ens_normalize::EnsError>(())
```

Core API:

- `ens_normalize(name) -> Result<String>`
- `ens_beautify(name) -> Result<String>`
- `ens_normalize_fragment(fragment, decompose) -> Result<String>`
- `ens_split(name, preserve_emoji) -> Vec<Label>`
- `ens_tokenize(name) -> Vec<Token>`
- `nfc(cps)` and `nfd(cps)` using the embedded Unicode data

## Verification

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

The Rust tests reuse the upstream fixture files in `tests/fixtures`.

For the JS/WASM compatibility harness, install `wasm-pack` and run:

```sh
cargo install wasm-pack --locked
scripts/test-js-compat.sh
```

## Benchmarks

```sh
cargo bench
```

To compare against `ens-normalize.js` after running `cargo bench`:

```sh
ENS_NORMALIZE_JS_DIR=/path/to/ens-normalize.js scripts/bench-js-compare.mjs
```

## Updating Upstream Data

To refresh from `ens-normalize.js`:

```sh
scripts/update-upstream.sh main
```

The script runs the upstream JS test suite, copies the derived data and
fixtures into this repo, then runs the Rust tests.
