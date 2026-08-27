import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { availableMonitors, getCurrentWindow, PhysicalPosition } from "@tauri-apps/api/window";
import { AlertTriangle, Sparkles } from "lucide-react";
import { quotaLabel } from "./usageFormatting";
import { calculateEdgeDock, clampWidgetPosition, interpolateWidgetPosition, type WidgetEasing, type WidgetSide } from "./widgetGeometry";

type DockMode = "free" | "edge" | "off";
type WidgetState = { currentMode: "official" | "channel" | "unknown"; currentChannelId?: string; currentProviderId: string; dockMode: DockMode };
type UsageWindow = { remainingPercent: number; durationMinutes?: number; resetsAt?: number };
type UsageSnapshot = { kind: "official" | "channel"; fiveHour?: UsageWindow; weekly?: UsageWindow; remainingBalance?: number; balanceLabel?: string; updatedAt: number };
type EdgeState = { side: WidgetSide; exposed: PhysicalPosition; hidden: PhysicalPosition };

const widget = getCurrentWindow();
const EDGE_POINTS = 48;
const VISIBLE_POINTS = 10;

export default function UsageWidget() {
  const [mode, setMode] = useState<DockMode>("off");
  const [usage, setUsage] = useState<UsageSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [resetTooltip, setResetTooltip] = useState<string | null>(null);
  const edge = useRef<EdgeState | null>(null);
  const suppressMoveUntil = useRef(0);
  const moveTimer = useRef<number | null>(null);
  const hideTimer = useRef<number | null>(null);
  const animationToken = useRef(0);

  const updateMode = useCallback((nextMode: DockMode) => {
    if (nextMode !== "edge") {
      const previousEdge = edge.current;
      edge.current = null;
      if (hideTimer.current) window.clearTimeout(hideTimer.current);
      if (previousEdge && nextMode === "free") {
        void animatePosition(previousEdge.exposed, 220, "reveal");
      }
    }
    setMode(nextMode);
  }, []);

  const load = useCallback(async () => {
    try {
      const state = await invoke<WidgetState>("get_widget_state");
      updateMode(state.dockMode);
      if (state.dockMode === "off") return;
      const channelId = state.currentMode === "official" ? "official" : state.currentChannelId;
      if (!channelId) throw new Error("无法识别当前渠道");
      setUsage(await invoke<UsageSnapshot>("get_usage", { channelId }));
      setError(null);
    } catch (reason) { setError(String(reason)); }
  }, [updateMode]);

  useEffect(() => {
    void load();
    const usageTimer = window.setInterval(() => void load(), 10_000);
    const modeTimer = window.setInterval(async () => {
      try {
        const state = await invoke<WidgetState>("get_widget_state");
        updateMode(state.dockMode);
      } catch { /* keep the last usable state */ }
    }, 2_000);
    return () => { window.clearInterval(usageTimer); window.clearInterval(modeTimer); };
  }, [load, updateMode]);

  const evaluateEdge = useCallback(async (position: PhysicalPosition) => {
    if (Date.now() < suppressMoveUntil.current) return;
    if (mode === "off") return;
    const size = await widget.outerSize();
    const monitors = await availableMonitors();
    const centerX = position.x + size.width / 2;
    const centerY = position.y + size.height / 2;
    const monitor = monitors.find((item) => centerX >= item.workArea.position.x && centerX <= item.workArea.position.x + item.workArea.size.width && centerY >= item.workArea.position.y && centerY <= item.workArea.position.y + item.workArea.size.height) ?? monitors[0];
    if (!monitor) return;
    const area = monitor.workArea;
    const clamped = clampWidgetPosition(position, size, area);
    if (clamped.x !== position.x || clamped.y !== position.y) {
      suppressMoveUntil.current = Date.now() + 500;
      await widget.setPosition(new PhysicalPosition(clamped.x, clamped.y));
    }
    await invoke("save_widget_position", { x: clamped.x, y: clamped.y });
    if (mode === "free") return;
    const dock = calculateEdgeDock(clamped, size, area, monitor.scaleFactor, EDGE_POINTS, VISIBLE_POINTS);
    if (!dock) { edge.current = null; return; }
    const exposed = new PhysicalPosition(dock.exposed.x, dock.exposed.y);
    const hidden = new PhysicalPosition(dock.hidden.x, dock.hidden.y);
    edge.current = { side: dock.side, exposed, hidden };
    await invoke("save_widget_position", { x: exposed.x, y: exposed.y });
    await animatePosition(hidden, 180, "hide");
  }, [mode]);

  useEffect(() => {
    if (mode !== "edge" || edge.current) return;
    void widget.outerPosition().then((position) => evaluateEdge(position)).catch(() => { /* retry after the next move */ });
  }, [mode, evaluateEdge]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void widget.onMoved(({ payload }) => {
      if (moveTimer.current) window.clearTimeout(moveTimer.current);
      moveTimer.current = window.setTimeout(() => void evaluateEdge(payload), 180);
    }).then((stop) => { unlisten = stop; });
    return () => { unlisten?.(); if (moveTimer.current) window.clearTimeout(moveTimer.current); };
  }, [evaluateEdge]);

  async function reveal() {
    if (hideTimer.current) window.clearTimeout(hideTimer.current);
    if (!edge.current) return;
    await animatePosition(edge.current.exposed, 220, "reveal");
  }

  function scheduleHide() {
    if (mode !== "edge" || !edge.current) return;
    if (hideTimer.current) window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(async () => {
      if (!edge.current) return;
      await animatePosition(edge.current.hidden, 180, "hide");
    }, 650);
  }

  async function beginDrag() {
    animationToken.current += 1;
    if (edge.current) {
      suppressMoveUntil.current = Date.now() + 500;
      await widget.setPosition(edge.current.exposed);
    }
    edge.current = null;
    await widget.startDragging();
  }

  async function animatePosition(target: PhysicalPosition, duration: number, easing: WidgetEasing) {
    const token = ++animationToken.current;
    const from = await widget.outerPosition();
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      suppressMoveUntil.current = Date.now() + 350;
      await widget.setPosition(target);
      return;
    }
    const started = performance.now();
    suppressMoveUntil.current = Date.now() + duration + 400;
    while (token === animationToken.current) {
      const progress = Math.min(1, (performance.now() - started) / duration);
      const point = interpolateWidgetPosition(from, target, progress, easing);
      await widget.setPosition(new PhysicalPosition(point.x, point.y));
      if (progress >= 1) break;
      await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
    }
  }

  return <div className={`usage-widget ${error ? "has-error" : ""}`} onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); void beginDrag(); }} onClick={(event) => { event.preventDefault(); event.stopPropagation(); }} onPointerEnter={() => void reveal()} onPointerLeave={scheduleHide} role="status" aria-live="polite">
    <span className="widget-logo"><img src="/modelay-logo.png" alt="" /></span>
    {usage?.kind === "official" ? <>
      <Metric label={quotaLabel(usage.fiveHour, "short", true)} value={percent(usage.fiveHour)} reset={usage.fiveHour?.resetsAt} onTooltip={setResetTooltip} />
      <span className="widget-divider" />
      <Metric label={quotaLabel(usage.weekly, "weekly", true)} value={percent(usage.weekly)} reset={usage.weekly?.resetsAt} onTooltip={setResetTooltip} />
    </> : usage ? <div className="widget-balance" title={usage.balanceLabel ?? "可用余额"}><small>{usage.balanceLabel ?? "余额"}</small><b>{formatBalance(usage.remainingBalance)}</b></div> : <div className="widget-loading" title={error ?? "正在读取额度"}>{error ? compactError(error) : "正在读取"}</div>}
    {error && <span className="widget-error" title={error}><AlertTriangle size={11} /></span>}
    {resetTooltip && <span className="widget-reset-tooltip">{resetTooltip}</span>}
  </div>;
}

function Metric({ label, value, reset, onTooltip }: { label: string; value: string; reset?: number; onTooltip: (value: string | null) => void }) {
  const resetText = reset ? new Date(reset * 1000).toLocaleString("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }) : "未知";
  const title = `${label}额度重置时间：${resetText}`;
  return <span className="widget-metric" title={title} aria-label={title} onPointerEnter={() => onTooltip(`${label}重置 · ${resetText}`)} onPointerLeave={() => onTooltip(null)}><small>{label}</small><b>{value}</b></span>;
}

function percent(value?: UsageWindow) { return value ? `${Math.round(value.remainingPercent)}%` : "—"; }
function formatBalance(value?: number) { return value == null ? "—" : value.toLocaleString(undefined, { maximumFractionDigits: 2 }); }
function compactError(value: string) { return value.replace(/^Error:\s*/, "").slice(0, 42); }
