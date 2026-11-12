import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const workflow = readFileSync(join(root, ".github/workflows/release.yml"), "utf8");
const manifest = JSON.parse(readFileSync(join(root, "bindings/ts/package.json"), "utf8"));
const plan = workflow.match(/        run: \|\n((?:          .*\n|\n)+)/)?.[1]
  .replace(/^          /gm, "");

test("release documentation uses the npm package name and matching tag", () => {
  const readme = readFileSync(join(root, "README.md"), "utf8");
  const npmRow = readme.split("\n").find((line) => line.includes("| npm |"));
  assert.equal(npmRow,
    `| \`${manifest.name}-vX.Y.Z\` | \`${manifest.name}\` | npm | \`bindings/ts/package.json\` |`);
  assert.ok(readme.includes(`- **npm** (owner of \`${manifest.name}\`):`),
    "trusted publisher setup must name the npm package");
});

test("npm releases accept the package tag and route it to the npm build", () => {
  assert.ok(workflow.includes(`- "${manifest.name}-v*"`));
  assert.match(workflow, new RegExp(`options: \\[[^\\n]*\\b${manifest.name}\\b`));
  assert.ok(workflow.includes(`if: needs.plan.outputs.package == '${manifest.name}'`));
  assert.ok(plan, "release plan shell script is present");
  const dir = mkdtempSync(join(tmpdir(), "etassia-release-plan-"));
  try {
    for (const event of ["push", "workflow_dispatch"]) {
      const output = join(dir, event);
      const result = spawnSync("bash", ["-c", plan], {
        cwd: root,
        encoding: "utf8",
        env: { ...process.env, EVENT: event, INPUT_PACKAGE: manifest.name,
          INPUT_PUBLISH: "true", REF_TYPE: "tag",
          REF_NAME: `${manifest.name}-v${manifest.version}`, GITHUB_OUTPUT: output },
      });
      assert.equal(result.status, 0, result.stderr);
      assert.equal(readFileSync(output, "utf8"), `package=${manifest.name}\npublish=true\n`);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("npm publish selects the tarball produced by npm pack", () => {
  const command = workflow.match(/        run: (npm publish .*)/)?.[1];
  assert.ok(command, "npm publish command is present");
  const dir = mkdtempSync(join(tmpdir(), "etassia-release-publish-"));
  try {
    mkdirSync(join(dir, "tarball"));
    const tarball = `./tarball/${manifest.name}-${manifest.version}.tgz`;
    writeFileSync(join(dir, tarball), "");
    // Intercept npm so this executes the workflow's glob without publishing.
    const result = spawnSync("bash", ["-c",
      `npm() { printf '%s\\n' "$@"; }; ${command}`], { cwd: dir, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(result.stdout.trim().split("\n"),
      ["publish", tarball, "--provenance", "--access", "public"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
