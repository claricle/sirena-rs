// Runs every golden case through the wasm build and byte-compares with golden/*.svg.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { render, detect, UnportedError, EtassiaError } from "../dist/index.js";

const golden = process.env.SIRENA_GOLDEN_DIR ?? fileURLToPath(new URL("../../../golden", import.meta.url));
const manifest = JSON.parse(readFileSync(join(golden, "manifest.json"), "utf8"));
const sha = (b) => createHash("sha256").update(b).digest("hex");

test("golden corpus is byte-identical to Ruby", () => {
  const counts = {};
  const failures = [];
  for (const c of manifest.cases) {
    const src = readFileSync(join(golden, c.case));
    assert.equal(sha(src), c.source_sha256, `${c.case} source`);
    const want = readFileSync(join(golden, c.case.replace(/\.mmd$/, ".svg")));
    const e = (counts[c.type] ??= { match: 0, mismatch: 0, unported: 0 });
    try {
      const got = Buffer.from(render(src.toString("utf8")), "utf8");
      if (got.equals(want)) e.match++;
      else { e.mismatch++; failures.push(c.case); }
    } catch (err) {
      if (err instanceof UnportedError) e.unported++;
      else { e.mismatch++; failures.push(`${c.case}: ${err.message}`); }
    }
  }
  console.log(JSON.stringify(counts, null, 1));
  const tot = Object.values(counts).reduce((a, c) => ({ match: a.match + c.match, mismatch: a.mismatch + c.mismatch, unported: a.unported + c.unported }), { match: 0, mismatch: 0, unported: 0 });
  console.log("TOTAL", JSON.stringify(tot), "cases", manifest.cases.length);
  assert.deepEqual(failures.slice(0, 5), []);
});

test("unported cases surface as UnportedError", () => {
  const unported = manifest.cases.filter((c) => c.type === "error");
  assert.ok(unported.length > 0);
  let seen = 0;
  for (const c of unported) {
    try { render(readFileSync(join(golden, c.case), "utf8")); } catch (e) { if (e instanceof UnportedError) seen++; }
  }
  assert.ok(seen > 0, "expected at least one UnportedError from the error/ cases");
});

test("undetectable input throws EtassiaError", () => {
  assert.throws(() => detect("not a diagram at all"), EtassiaError);
});

test("detect names the diagram type", () => {
  assert.equal(detect('pie\n  "a": 1\n'), "pie");
});

test("repeated calls are stable", () => {
  const src = 'pie\n  "a": 1\n';
  assert.equal(render(src), render(src));
});
