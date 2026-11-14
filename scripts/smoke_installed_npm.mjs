// Smoke an INSTALLED etassia tarball: render one golden case, byte-compare.
// usage: node smoke_installed_npm.mjs <golden.mmd> <golden.svg>   (cwd = clean project with etassia installed)
import { readFileSync } from "node:fs";
import { render } from "etassia";

const [mmd, svg] = process.argv.slice(2);
const got = Buffer.from(render(readFileSync(mmd, "utf8")), "utf8");
const want = readFileSync(svg);
if (!got.equals(want)) {
  console.error(`MISMATCH: rendered ${got.length} bytes, golden ${want.length} bytes`);
  process.exit(1);
}
console.log(`OK ${mmd}: byte-identical (${got.length} bytes)`);
