import assert from "node:assert/strict";
import test from "node:test";
import { execFileSync } from "node:child_process";

test("uses NSIS only for the current non-numeric alpha prerelease", () => {
  const bundles = execFileSync(process.execPath, ["scripts/windows-bundles.mjs"], { encoding: "utf8" });
  assert.equal(bundles, "nsis");
});
