# sirena-ts

TypeScript binding for the Sirena Rust runtime: `crates/sirena-wasm` (sirena-core on
`wasm32-unknown-unknown`, no wasm-bindgen glue) plus a thin loader. All rendering,
including number formatting, happens in Rust; JS only moves bytes.

```ts
import { render, detect, UnportedError, SirenaError } from "sirena-ts";
try { const svg = render("pie\n  \"a\": 1\n"); }
catch (e) { if (e instanceof UnportedError) { /* use the Ruby fallback */ } }
```

`npm test` builds the wasm (needs the `wasm32-unknown-unknown` rust target), then
renders every `golden/` case (or `SIRENA_GOLDEN_DIR`) and byte-compares it with Ruby's SVG.
Not published; version unchanged.
