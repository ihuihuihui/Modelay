import assert from "node:assert/strict";
import test from "node:test";
import { classifyUpdaterError, downloadPercent } from "../src/updateState.ts";

test("calculates bounded updater download progress", () => {
  assert.equal(downloadPercent(25, 100), 25);
  assert.equal(downloadPercent(140, 100), 100);
  assert.equal(downloadPercent(-1, 100), 0);
  assert.equal(downloadPercent(25), null);
});

test("distinguishes an unconfigured updater from a network failure", () => {
  assert.deepEqual(classifyUpdaterError("Updater does not have any endpoints set."), {
    phase: "unconfigured",
    message: "发布源尚未配置",
  });
  assert.equal(classifyUpdaterError("request timed out").message, "检查更新超时，请稍后重试");
});

test("never encourages installation after signature verification fails", () => {
  assert.deepEqual(classifyUpdaterError("Signature verification failed"), {
    phase: "error",
    message: "更新签名验证失败，已拒绝安装",
  });
});
