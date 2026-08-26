import assert from "node:assert/strict";
import test from "node:test";
import { quotaLabel } from "../src/usageFormatting.ts";

test("keeps the familiar five-hour and weekly labels", () => {
  assert.equal(quotaLabel({ durationMinutes: 300 }, "short"), "5 小时");
  assert.equal(quotaLabel({ durationMinutes: 300 }, "short", true), "5h");
  assert.equal(quotaLabel({ durationMinutes: 10_080 }, "weekly"), "周额度");
  assert.equal(quotaLabel({ durationMinutes: 10_080 }, "weekly", true), "周");
});

test("does not mislabel other short quota windows as five hours", () => {
  assert.equal(quotaLabel({ durationMinutes: 15 }, "short"), "15 分钟");
  assert.equal(quotaLabel({ durationMinutes: 15 }, "short", true), "15m");
  assert.equal(quotaLabel({ durationMinutes: 60 }, "short"), "1 小时");
  assert.equal(quotaLabel({ durationMinutes: 1_440 }, "short"), "1 天");
});

test("uses stable compatibility labels when duration metadata is absent", () => {
  assert.equal(quotaLabel(undefined, "short"), "5 小时");
  assert.equal(quotaLabel(undefined, "weekly", true), "周");
});
