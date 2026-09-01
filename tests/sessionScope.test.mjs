import test from "node:test";
import assert from "node:assert/strict";
import { resolveSessionScope } from "../src/sessionScope.ts";

test("keeps the requested task scope when switching channels", () => {
  assert.equal(resolveSessionScope(false, "recent5"), "recent5");
  assert.equal(resolveSessionScope(false, "single"), "single");
  assert.equal(resolveSessionScope(false, "all"), "all");
});

test("preserves fine-grained scope when reapplying the current channel", () => {
  assert.equal(resolveSessionScope(true, "recent5"), "recent5");
  assert.equal(resolveSessionScope(true, "single"), "single");
});
