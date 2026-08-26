import assert from "node:assert/strict";
import test from "node:test";
import { resolveManualModelFallback } from "../src/modelSelection.ts";

test("allows a manually configured model when server validation is disabled", () => {
  const result = resolveManualModelFallback("  custom-model  ", false, "HTTP 404");
  assert.equal(result?.model.id, "custom-model");
  assert.equal(result?.model.isDefault, true);
  assert.match(result?.notice ?? "", /未校验服务端模型列表/);
});

test("does not bypass required model validation", () => {
  assert.equal(resolveManualModelFallback("custom-model", true, "HTTP 404"), null);
  assert.equal(resolveManualModelFallback("   ", false, "HTTP 404"), null);
});
