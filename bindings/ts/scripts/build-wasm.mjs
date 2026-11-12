// Build crates/etassia-wasm for wasm32-unknown-unknown and copy it to dist/sirena.wasm.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
execFileSync("cargo", ["build", "-p", "etassia-wasm", "--target", "wasm32-unknown-unknown", "--release"], {
  cwd: root,
  stdio: "inherit",
});
const out = join(dirname(fileURLToPath(import.meta.url)), "..", "dist");
mkdirSync(out, { recursive: true });
copyFileSync(join(root, "target/wasm32-unknown-unknown/release/etassia_wasm.wasm"), join(out, "sirena.wasm"));
