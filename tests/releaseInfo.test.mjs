import test from "node:test";
import assert from "node:assert/strict";
import { currentReleaseInfo } from "../src/releaseInfo.ts";

test("provides a clear current-version summary", () => {
  assert.match(currentReleaseInfo.summary, /Beta 20/);
  assert.ok(currentReleaseInfo.summary.length >= 20);
});

test("lists the user-visible changes in Chinese", () => {
  assert.ok(currentReleaseInfo.changes.length >= 4);
  assert.ok(currentReleaseInfo.changes.every((item) => /[\u4e00-\u9fff]/.test(item)));
});
