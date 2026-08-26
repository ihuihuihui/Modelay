export type QuotaWindowLike = { durationMinutes?: number };

export function quotaLabel(
  window: QuotaWindowLike | undefined,
  fallback: "short" | "weekly",
  compact = false,
) {
  const minutes = window?.durationMinutes;
  if (!minutes || !Number.isFinite(minutes) || minutes <= 0) {
    return fallback === "weekly" ? (compact ? "周" : "周额度") : (compact ? "5h" : "5 小时");
  }
  if (minutes === 10_080) return compact ? "周" : "周额度";
  if (minutes % 1_440 === 0) {
    const days = minutes / 1_440;
    return compact ? `${days}d` : `${days} 天`;
  }
  if (minutes % 60 === 0) {
    const hours = minutes / 60;
    return compact ? `${hours}h` : `${hours} 小时`;
  }
  return compact ? `${minutes}m` : `${minutes} 分钟`;
}
