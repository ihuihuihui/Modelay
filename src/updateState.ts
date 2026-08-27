export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "latest"
  | "downloading"
  | "installing"
  | "unconfigured"
  | "error";

export function downloadPercent(downloaded: number, total?: number): number | null {
  if (!total || total <= 0) return null;
  return Math.max(0, Math.min(100, Math.round((downloaded / total) * 100)));
}

export function classifyUpdaterError(reason: unknown): { phase: "unconfigured" | "error"; message: string } {
  const raw = String(reason);
  const normalized = raw.toLowerCase();
  if (normalized.includes("does not have any endpoints") || normalized.includes("no endpoints")) {
    return { phase: "unconfigured", message: "发布源尚未配置" };
  }
  if (normalized.includes("timed out") || normalized.includes("timeout")) {
    return { phase: "error", message: "检查更新超时，请稍后重试" };
  }
  if (normalized.includes("signature") || normalized.includes("signature verification")) {
    return { phase: "error", message: "更新签名验证失败，已拒绝安装" };
  }
  return { phase: "error", message: raw || "检查更新失败" };
}
