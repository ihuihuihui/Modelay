import test from "node:test";
import assert from "node:assert/strict";
import { resolveSessionScope } from "../src/sessionScope.ts";

test("preserves existing tasks when switching to another channel", () => {
  assert.equal(resolveSessionScope(false, "recent5"), "none");
  assert.equal(resolveSessionScope(false, "single"), "none");
});

test("preserves fine-grained scope when reapplying the current channel", () => {
  assert.equal(resolveSessionScope(true, "recent5"), "recent5");
  assert.equal(resolveSessionScope(true, "single"), "single");
});
