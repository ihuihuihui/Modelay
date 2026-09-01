import test from "node:test";
import assert from "node:assert/strict";
import { resolveSessionScope, switchRequiresThread } from "../src/sessionScope.ts";

test("does not rewrite tasks for smart continuation or a plain switch", () => {
  assert.equal(resolveSessionScope(false, "single", "smart"), "none");
  assert.equal(resolveSessionScope(false, "all", "switchOnly"), "none");
});

test("preserves explicit migration and current-channel scopes", () => {
  assert.equal(resolveSessionScope(false, "single", "migrate"), "single");
  assert.equal(resolveSessionScope(false, "all", "migrate"), "all");
  assert.equal(resolveSessionScope(true, "recent5", "smart"), "recent5");
});

test("requires a selected task only when the chosen workflow needs one", () => {
  assert.equal(switchRequiresThread(false, "single", "smart"), true);
  assert.equal(switchRequiresThread(false, "all", "switchOnly"), false);
  assert.equal(switchRequiresThread(false, "single", "migrate"), true);
  assert.equal(switchRequiresThread(false, "all", "migrate"), false);
});
