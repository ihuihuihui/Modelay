import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  check,
  type DownloadEvent,
  type Update,
} from "@tauri-apps/plugin-updater";
import {
  Activity,
  AlertTriangle,
  ArrowRight,
  Check,
  ChevronRight,
  CircleHelp,
  Copy,
  Database,
  Download,
  FileText,
  FolderOpen,
  KeyRound,
  LogIn,
  Pencil,
  Plus,
  RefreshCw,
  Settings2,
  ShieldCheck,
  Sparkles,
  Trash2,
  Wifi,
  X,
  Moon,
  Sun,
} from "lucide-react";
import { resolveManualModelFallback } from "./modelSelection";
import {
  classifyUpdaterError,
  downloadPercent,
  type UpdatePhase,
} from "./updateState";
import { quotaLabel } from "./usageFormatting";
import {
  resolveSessionScope,
  switchRequiresThread,
  type CrossChannelMode,
  type SessionScope,
} from "./sessionScope";
import { currentReleaseInfo } from "./releaseInfo";

type Channel = {
  id: string;
  name: string;
  baseUrl: string;
  model: string;
  reasoningEffort: string;
  modelsPath: string;
  usagePath: string;
  validatesModelList: boolean;
  isBuiltIn: boolean;
  hasSecret?: boolean;
};
type AppState = {
  platform: string;
  currentMode: "official" | "channel" | "unknown";
  currentChannelId?: string;
  currentProviderId: string;
  currentModel: string;
  currentReasoningEffort: string;
  officialLoggedIn: boolean;
  configExists: boolean;
  configConformant: boolean;
  imageSkill: string;
  channels: Channel[];
  officialModel: string;
  officialReasoningEffort: string;
  backupDirectory: string;
  dockMode: "free" | "edge" | "off";
  widgetPosition?: { x: number; y: number };
};
type ModelInfo = {
  id: string;
  displayName: string;
  description: string;
  isDefault: boolean;
  supportedReasoningEfforts: string[];
};
type CheckResult = {
  title: string;
  detail: string;
  state: "passed" | "warning" | "failed";
};
type SwitchReport = {
  channelId: string;
  providerId: string;
  model: string;
  reasoningEffort: string;
  sessionScope: SessionScope;
  imageSkill: string;
  backupPath: string;
  needsRestart: boolean;
  checks: CheckResult[];
};
type UsageWindow = {
  remainingPercent: number;
  durationMinutes?: number;
  resetsAt?: number;
};
type UsageSnapshot = {
  kind: "official" | "channel";
  channelId: string;
  planName?: string;
  fiveHour?: UsageWindow;
  weekly?: UsageWindow;
  remainingBalance?: number;
  balanceLabel?: string;
  creditsBalance?: string;
  updatedAt: number;
};
type Draft = Channel & { secret: string };
type InfoPanel = "help" | "settings" | null;
type ThreadHealth = {
  threadId: string;
  title: string;
  cwd: string;
  providerId: string;
  model: string;
  reasoningEffort: string;
  tokensUsed: number;
  latestInputTokens: number;
  todayMessageCount: number;
  todayRolloutBytes: number;
  riskLevel: "healthy" | "warning" | "critical";
  riskLabel: string;
  riskReasons: string[];
  latestUserRequest?: string;
};
type ThreadSummary = {
  threadId: string;
  title: string;
  cwd: string;
  providerId: string;
  originalProviderId?: string;
  model: string;
  updatedAtMs: number;
  issue?: string;
};
type HandoffReport = {
  sourceThreadId: string;
  newThreadId: string;
  title: string;
  cwd: string;
  messageCount: number;
  referencedPaths: string[];
  riskLevel: string;
};

const officialChannel: Channel = {
  id: "official",
  name: "OpenAI 官方",
  baseUrl: "ChatGPT 账号登录",
  model: "",
  reasoningEffort: "medium",
  modelsPath: "",
  usagePath: "",
  validatesModelList: true,
  isBuiltIn: true,
};
const fallbackReasoningEfforts = ["low", "medium", "high"];
const commonChannelModels: ModelInfo[] = [
  "gpt-5.6-sol",
  "gpt-5.6-terra",
  "gpt-5.6-luna",
  "gpt-5.5",
  "gpt-5.4",
  "gpt-5.3-codex",
].map((id) => ({
  id,
  displayName: id,
  description: "常用模型",
  isDefault: false,
  supportedReasoningEfforts: fallbackReasoningEfforts,
}));
const reasoningLabels: Record<string, string> = {
  none: "即时",
  low: "快速",
  medium: "平衡（推荐）",
  high: "深度",
  xhigh: "极深",
  max: "最大",
};
const sessionScopeLabels: Record<SessionScope, string> = {
  none: "不改写旧任务",
  recent5: "最近活动的 5 个任务",
  all: "覆盖全部旧任务",
  single: "指定一个会话 ID",
};
const crossChannelModeLabels: Record<CrossChannelMode, string> = {
  smart: "智能续接",
  switchOnly: "仅切换渠道",
  migrate: "原会话迁移",
};

function App() {
  const [state, setState] = useState<AppState | null>(null);
  const [selectedId, setSelectedId] = useState("official");
  const [selectedModel, setSelectedModel] = useState("");
  const [selectedReasoningEffort, setSelectedReasoningEffort] =
    useState("medium");
  const [sessionScope, setSessionScope] = useState<SessionScope>("single");
  const [crossChannelMode, setCrossChannelMode] =
    useState<CrossChannelMode>("migrate");
  const [threadId, setThreadId] = useState("");
  const [switchThreadHealth, setSwitchThreadHealth] =
    useState<ThreadHealth | null>(null);
  const [switchHandoffReport, setSwitchHandoffReport] =
    useState<HandoffReport | null>(null);
  const [handoffThreadId, setHandoffThreadId] = useState("");
  const [threadOptions, setThreadOptions] = useState<ThreadSummary[]>([]);
  const [threadsLoading, setThreadsLoading] = useState(false);
  const [threadListError, setThreadListError] = useState<string | null>(null);
  const [threadHealth, setThreadHealth] = useState<ThreadHealth | null>(null);
  const [handoffReport, setHandoffReport] = useState<HandoffReport | null>(
    null,
  );
  const [handoffError, setHandoffError] = useState<string | null>(null);
  const [handoffBusy, setHandoffBusy] = useState(false);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const [modelNotice, setModelNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("正在读取真实配置…");
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [report, setReport] = useState<SwitchReport | null>(null);
  const [restartOpen, setRestartOpen] = useState(false);
  const [switchInProgress, setSwitchInProgress] = useState(false);
  const [switchConfirmOpen, setSwitchConfirmOpen] = useState(false);
  const [usage, setUsage] = useState<UsageSnapshot | null>(null);
  const [usageError, setUsageError] = useState<string | null>(null);
  const [usageLoading, setUsageLoading] = useState(false);
  const [infoPanel, setInfoPanel] = useState<InfoPanel>(null);
  const [pendingDelete, setPendingDelete] = useState<Channel | null>(null);
  const [confirmSecretDelete, setConfirmSecretDelete] = useState(false);
  const [appVersion, setAppVersion] = useState("读取中");
  const [updatePhase, setUpdatePhase] = useState<UpdatePhase>("idle");
  const [updateMessage, setUpdateMessage] = useState("尚未检查更新");
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);
  const [updateNotes, setUpdateNotes] = useState<string | null>(null);
  const [updateProgress, setUpdateProgress] = useState<number | null>(null);
  const [updateOpen, setUpdateOpen] = useState(false);
  const [showAllChanges, setShowAllChanges] = useState(false);
  const [theme, setTheme] = useState<"dark" | "light">(() => (localStorage.getItem("modelay-theme") as "dark" | "light") || "dark");
  const pendingUpdate = useRef<Update | null>(null);
  const startupUpdateCheck = useRef(false);
  const notifiedUpdateVersion = useRef<string | null>(null);
  const UPDATE_INTERVAL_MS = 3 * 60 * 60 * 1000;

  useEffect(() => { localStorage.setItem("modelay-theme", theme); }, [theme]);

  const allChannels = useMemo(
    () => [officialChannel, ...(state?.channels ?? [])],
    [state],
  );
  const selected =
    allChannels.find((channel) => channel.id === selectedId) ?? officialChannel;
  const activeId =
    state?.currentMode === "official" ? "official" : state?.currentChannelId;
  const active = allChannels.find((channel) => channel.id === activeId);
  const selectedThread = threadOptions.find(
    (thread) => thread.threadId === handoffThreadId,
  );
  const selectedModelInfo = models.find((model) => model.id === selectedModel);
  const availableReasoningEfforts = useMemo(() => {
    const advertised = selectedModelInfo?.supportedReasoningEfforts ?? [];
    const values = advertised.length ? advertised : fallbackReasoningEfforts;
    return values.includes(selectedReasoningEffort)
      ? values
      : [...values, selectedReasoningEffort];
  }, [selectedModelInfo, selectedReasoningEffort]);

  const loadState = useCallback(async () => {
    try {
      const next = await invoke<AppState>("get_app_state");
      setState(next);
      const actual =
        next.currentMode === "official"
          ? "official"
          : (next.currentChannelId ?? "official");
      setSelectedId((current) =>
        current === "official" && actual !== "official" ? actual : current,
      );
      setError(null);
      setMessage("已读取当前 Codex 配置");
    } catch (reason) {
      setError(String(reason));
      setMessage("无法读取当前配置");
    }
  }, []);

  useEffect(() => {
    void loadState();
  }, [loadState]);

  const loadThreads = useCallback(async () => {
    setThreadsLoading(true);
    try {
      setThreadOptions(await invoke<ThreadSummary[]>("list_user_threads"));
      setThreadListError(null);
    } catch (reason) {
      setThreadListError(String(reason));
    } finally {
      setThreadsLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadThreads();
  }, [loadThreads]);

  const checkForUpdates = useCallback(async (manual = false) => {
    setUpdatePhase("checking");
    setUpdateMessage("正在安全检查新版本…");
    try {
      const next = await check({ timeout: 15_000 });
      if (!next) {
        pendingUpdate.current = null;
        setUpdateVersion(null);
        setUpdateNotes(null);
        setUpdatePhase("latest");
        setUpdateMessage("当前已是最新版本");
        if (manual) setMessage("Modelay 当前已是最新版本");
        return;
      }
      pendingUpdate.current = next;
      setUpdateVersion(next.version);
      setUpdateNotes(next.body ?? null);
      setUpdatePhase("available");
      setUpdateMessage(`发现新版本 ${next.version}`);
      if (notifiedUpdateVersion.current !== next.version || manual) {
        setUpdateOpen(true);
        notifiedUpdateVersion.current = next.version;
      }
    } catch (reason) {
      const classified = classifyUpdaterError(reason);
      setUpdatePhase(classified.phase);
      setUpdateMessage(classified.message);
      if (manual && classified.phase === "error") setError(classified.message);
    }
  }, []);

  useEffect(() => {
    void getVersion()
      .then(setAppVersion)
      .catch(() => undefined);
    if (startupUpdateCheck.current) return;
    startupUpdateCheck.current = true;
    const timer = window.setTimeout(() => void checkForUpdates(false), 4_000);
    const interval = window.setInterval(
      () => void checkForUpdates(false),
      UPDATE_INTERVAL_MS,
    );
    return () => {
      window.clearTimeout(timer);
      window.clearInterval(interval);
    };
  }, [checkForUpdates]);

  const loadModels = useCallback(
    async (
      channelId: string,
      fallbackModel: string,
      validatesModelList: boolean,
    ) => {
      setModelsLoading(true);
      setModelError(null);
      setModelNotice(null);
      setModels([]);
      try {
        const result = await invoke<ModelInfo[]>("list_models", { channelId });
        const fallback = fallbackModel.trim();
        const manual = !result.length
          ? resolveManualModelFallback(fallback, validatesModelList)
          : null;
        const usable = manual ? [manual.model] : result;
        setModels(usable);
        const preferred =
          fallbackModel ||
          result.find((model) => model.isDefault)?.id ||
          result[0]?.id ||
          "";
        setSelectedModel(
          usable.some((model) => model.id === preferred)
            ? preferred
            : (usable[0]?.id ?? ""),
        );
        if (manual) setModelNotice(manual.notice);
      } catch (reason) {
        const manual = resolveManualModelFallback(
          fallbackModel,
          validatesModelList,
          String(reason),
        );
        if (manual) {
          setModels([manual.model]);
          setSelectedModel(manual.model.id);
          setModelNotice(manual.notice);
        } else {
          setModelError(String(reason));
          setSelectedModel(fallbackModel);
        }
      } finally {
        setModelsLoading(false);
      }
    },
    [],
  );

  useEffect(() => {
    if (!state) return;
    const selectedChannel = state.channels.find(
      (channel) => channel.id === selectedId,
    );
    const fallback =
      selectedId === "official"
        ? state.officialModel
        : (selectedChannel?.model ?? "");
    const preferredEffort =
      activeId === selectedId && fallback === state.currentModel
        ? state.currentReasoningEffort
        : selectedId === "official"
          ? state.officialReasoningEffort
          : (selectedChannel?.reasoningEffort ?? "medium");
    setSelectedReasoningEffort(preferredEffort);
    void loadModels(
      selectedId,
      fallback,
      selectedId === "official" ||
        (selectedChannel?.validatesModelList ?? true),
    );
  }, [state, selectedId, activeId, loadModels]);

  useEffect(() => {
    const advertised = selectedModelInfo?.supportedReasoningEfforts ?? [];
    if (advertised.length && !advertised.includes(selectedReasoningEffort)) {
      setSelectedReasoningEffort(
        advertised.includes("medium") ? "medium" : advertised[0],
      );
    }
  }, [selectedModelInfo, selectedReasoningEffort]);

  const refreshUsage = useCallback(async () => {
    if (!activeId) return;
    setUsageLoading(true);
    try {
      setUsage(
        await invoke<UsageSnapshot>("get_usage", { channelId: activeId }),
      );
      setUsageError(null);
    } catch (reason) {
      setUsageError(String(reason));
    } finally {
      setUsageLoading(false);
    }
  }, [activeId]);

  useEffect(() => {
    void refreshUsage();
    const timer = window.setInterval(() => void refreshUsage(), 10_000);
    return () => window.clearInterval(timer);
  }, [refreshUsage]);

  async function openSwitchConfirmation() {
    if (!canSwitch) return;
    setError(null);
    setSwitchThreadHealth(null);
    if (requiresSwitchThread) {
      setBusy(true);
      setMessage("正在检查所选任务的跨渠道上下文风险…");
      try {
        setSwitchThreadHealth(
          await invoke<ThreadHealth>("get_thread_health", {
            threadId: threadId.trim(),
          }),
        );
      } catch (reason) {
        setError(String(reason));
        setMessage("无法检查所选任务，已停止切换");
        return;
      } finally {
        setBusy(false);
      }
    }
    setSwitchConfirmOpen(true);
  }

  async function switchChannel() {
    if (!selectedModel || modelError) return;
    const effectiveScope = resolveSessionScope(
      isCurrent,
      sessionScope,
      crossChannelMode,
    );
    if (requiresSwitchThread && !threadId.trim()) {
      setError("请输入需要更新的会话 ID。");
      return;
    }
    setSwitchConfirmOpen(false);
    setSwitchInProgress(true);
    setBusy(true);
    setError(null);
    setReport(null);
    setSwitchHandoffReport(null);
    setMessage("渠道切换进行中：正在备份、写入并验证配置…");
    try {
      const result = await invoke<SwitchReport>("switch_channel", {
        request: {
          channelId: selectedId,
          model: selectedModel,
          reasoningEffort: selectedReasoningEffort,
          sessionScope: effectiveScope,
          threadId: effectiveScope === "single" ? threadId.trim() : null,
          fastSwitch: true,
        },
      });
      setReport(result);
      let smartHandoff: HandoffReport | null = null;
      if (!isCurrent && crossChannelMode === "smart") {
        setMessage("目标渠道已生效，正在创建精简续接任务…");
        try {
          smartHandoff = await invoke<HandoffReport>("create_thread_handoff", {
            request: { threadId: threadId.trim() },
          });
          setSwitchHandoffReport(smartHandoff);
        } catch (reason) {
          setError(`渠道已切换，但智能续接失败：${String(reason)}`);
        }
      }
      setMessage(
        smartHandoff
          ? `智能续接任务 ${smartHandoff.newThreadId} 已创建，正在重启 Codex`
          : `渠道已切换到 ${selected.name}，正在重启 Codex`,
      );
      setSwitchInProgress(false);
      setBusy(false);
      try {
        await invoke("restart_chatgpt");
        setMessage("Codex 已重新打开，渠道切换完成");
      } catch (reason) {
        setError(`渠道已切换，但自动重启失败：${String(reason)}`);
        setRestartOpen(result.needsRestart);
      }
      await loadState();
      await refreshUsage();
    } catch (reason) {
      setError(String(reason));
      setMessage("切换失败，已尝试恢复原配置");
    } finally {
      setSwitchInProgress(false);
      setBusy(false);
    }
  }

  async function saveChannel() {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      const channel: Channel = { ...draft };
      delete (channel as Partial<Draft>).secret;
      const next = await invoke<AppState>("save_channel", {
        request: { channel, secret: draft.secret || null },
      });
      setState(next);
      setSelectedId(channel.id);
      setDraft(null);
      setMessage(`${channel.name} 已安全保存`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  async function deleteChannel(channel: Channel) {
    if (channel.isBuiltIn) return;
    setBusy(true);
    try {
      setState(
        await invoke<AppState>("delete_channel", { channelId: channel.id }),
      );
      setSelectedId("official");
      setPendingDelete(null);
      setMessage(`${channel.name} 已删除`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  async function deleteSecret() {
    if (!draft) return;
    setBusy(true);
    try {
      setState(
        await invoke<AppState>("delete_secret", { channelId: draft.id }),
      );
      setDraft({ ...draft, hasSecret: false, secret: "" });
      setConfirmSecretDelete(false);
      setMessage(`${draft.name} 的密钥已删除`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  async function loginOfficial() {
    setBusy(true);
    setMessage("请在浏览器中完成 OpenAI 登录…");
    setError(null);
    try {
      setState(await invoke<AppState>("login_official"));
      setMessage("OpenAI 官方账号登录有效");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  async function restartNow() {
    setRestartOpen(false);
    setBusy(true);
    setMessage("正在重启 Codex…");
    try {
      await invoke("restart_chatgpt");
      setMessage("Codex 已重新打开");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  async function copyText(text: string, label: string) {
    try {
      await navigator.clipboard.writeText(text);
      setMessage(`${label}已复制`);
      return;
    } catch {
      const textarea = document.createElement("textarea");
      textarea.value = text;
      textarea.setAttribute("readonly", "");
      textarea.style.position = "fixed";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.focus();
      textarea.select();
      const copied = document.execCommand("copy");
      textarea.remove();
      if (copied) {
        setMessage(`${label}已复制`);
      } else {
        setError(`无法复制${label}，请先点击 Modelay 窗口后重试。`);
      }
    }
  }

  async function installUpdate() {
    const next = pendingUpdate.current;
    if (!next) return;
    setUpdatePhase("downloading");
    setUpdateProgress(0);
    setUpdateMessage(`正在下载 ${next.version}…`);
    let downloaded = 0;
    let total: number | undefined;
    const onEvent = (event: DownloadEvent) => {
      if (event.event === "Started") {
        total = event.data.contentLength;
        setUpdateProgress(downloadPercent(downloaded, total));
      } else if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
        setUpdateProgress(downloadPercent(downloaded, total));
      } else {
        setUpdatePhase("installing");
        setUpdateProgress(100);
        setUpdateMessage("下载完成，正在验证签名并安装…");
      }
    };
    try {
      await next.downloadAndInstall(onEvent, { timeout: 120_000 });
      setUpdateMessage("更新安装完成，正在重启 Modelay…");
      await relaunch();
    } catch (reason) {
      const classified = classifyUpdaterError(reason);
      setUpdatePhase(classified.phase);
      setUpdateMessage(classified.message);
      setError(classified.message);
    }
  }

  async function setWidgetMode(mode: AppState["dockMode"]) {
    try {
      const next = await invoke<AppState>("set_widget_mode", { mode });
      setState(next);
      setMessage(
        mode === "free"
          ? "额度胶囊已设为自由悬浮"
          : mode === "edge"
            ? "额度胶囊已设为靠边隐藏"
            : "额度胶囊已关闭",
      );
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function inspectThread() {
    const value = handoffThreadId.trim();
    if (!value) {
      setError("请输入需要检查的会话 ID。");
      return;
    }
    setHandoffBusy(true);
    setThreadHealth(null);
    setHandoffReport(null);
    setHandoffError(null);
    setError(null);
    setMessage("正在读取当天会话记录并分析风险…");
    try {
      const result = await invoke<ThreadHealth>("get_thread_health", {
        threadId: value,
      });
      setThreadHealth(result);
      setMessage(
        result.riskLevel === "healthy"
          ? "会话检查完成，当前风险较低"
          : `会话检查完成：${result.riskLabel}`,
      );
    } catch (reason) {
      setError(String(reason));
      setMessage("无法检查该会话");
    } finally {
      setHandoffBusy(false);
    }
  }

  async function createHandoff() {
    if (!threadHealth) return;
    setHandoffBusy(true);
    setHandoffError(null);
    setError(null);
    setMessage("正在整理当天需求、项目资料和进度，并创建新任务…");
    try {
      const result = await invoke<HandoffReport>("create_thread_handoff", {
        request: { threadId: threadHealth.threadId },
      });
      setHandoffReport(result);
      setMessage(
        `续接任务 ${result.newThreadId} 已创建，交接摘要已写入且不会占用会话`,
      );
    } catch (reason) {
      const detail = String(reason);
      setHandoffError(detail);
      setError(detail);
      setMessage("续接任务创建失败，旧任务未受影响");
    } finally {
      setHandoffBusy(false);
    }
  }

  async function compactThread() {
    if (!threadHealth || handoffBusy) return;
    setHandoffBusy(true);
    setHandoffError(null);
    setError(null);
    setMessage("正在压缩原任务上下文，请稍候…");
    try {
      await invoke("compact_thread", { threadId: threadHealth.threadId });
      await inspectThread();
      setMessage("原任务上下文压缩完成，可以继续使用");
    } catch (reason) {
      const detail = String(reason);
      setHandoffError(detail);
      setError(detail);
      setMessage("原任务压缩失败，旧任务未被删除");
    } finally {
      setHandoffBusy(false);
    }
  }

  function editChannel(channel?: Channel) {
    const value = channel ?? {
      id: `channel-${crypto.randomUUID().slice(0, 8)}`,
      name: "",
      baseUrl: "",
      model: "",
      reasoningEffort: "medium",
      modelsPath: "/v1/models",
      usagePath: "/v1/usage",
      validatesModelList: true,
      isBuiltIn: false,
    };
    setDraft({ ...value, secret: "" });
  }

  const isCurrent = activeId === selectedId;
  const effectiveSessionScope = resolveSessionScope(
    isCurrent,
    sessionScope,
    crossChannelMode,
  );
  const requiresSwitchThread = switchRequiresThread(
    isCurrent,
    sessionScope,
    crossChannelMode,
  );
  const canSwitch =
    !!state &&
    !!selectedModel &&
    !busy &&
    !modelsLoading &&
    !modelError &&
    (!requiresSwitchThread || !!threadId.trim()) &&
    (selectedId === "official" ? state.officialLoggedIn : !!selected.hasSecret);

  return (
    <div className={`shell ${theme === "light" ? "theme-light" : "theme-dark"}`}>
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">
            <img src="/modelay-logo.png" alt="Modelay" />
          </div>
          <div>
            <div className="brand-name">Modelay</div>
            <div className="brand-sub">AI 渠道与额度管理器 · 4.0 Beta</div>
          </div>
        </div>
        <div className="top-actions">
          <span className="platform">
            <span className={`dot ${state?.configConformant ? "" : "warn"}`} />
            {state?.platform ?? "读取中"}
          </span>
          {updatePhase === "available" && (
            <button
              className="update-badge"
              title={`可更新至 ${updateVersion}`}
              onClick={() => setUpdateOpen(true)}
            >
              <Download size={13} />
              新版本
            </button>
          )}
          <button className="icon-btn" title={theme === "dark" ? "切换浅色模式" : "切换深色模式"} onClick={() => setTheme(theme === "dark" ? "light" : "dark")}>{theme === "dark" ? <Sun size={17} /> : <Moon size={17} />}</button>
          <button
            className="icon-btn"
            title="打开备份目录"
            onClick={() => void invoke("open_backup_folder")}
          >
            <FolderOpen size={17} />
          </button>
          <button
            className="icon-btn"
            title="帮助"
            onClick={() => setInfoPanel("help")}
          >
            <CircleHelp size={17} />
          </button>
          <button
            className="icon-btn"
            title="设置"
            onClick={() => setInfoPanel("settings")}
          >
            <Settings2 size={17} />
          </button>
        </div>
      </header>
      <main className="content">
        <section className="hero">
          <div>
            <p className="eyebrow">当前实际渠道</p>
            <h1>{active?.name ?? state?.currentProviderId ?? "正在检测"}</h1>
            <p className="hero-detail">
              {state
                ? `${state.currentProviderId} · ${state.currentModel || "未设置模型"} · ${reasoningLabels[state.currentReasoningEffort] ?? state.currentReasoningEffort}`
                : "正在读取 ~/.codex/config.toml"}{" "}
              {state && (
                <span
                  className={`status-pill ${state.configConformant ? "" : "warning"}`}
                >
                  <span
                    className={`dot ${state.configConformant ? "" : "warn"}`}
                  />
                  {state.configConformant ? "配置一致" : "配置需修复"}
                </span>
              )}
            </p>
          </div>
          <div className="hero-actions">
            <button
              className="ghost-btn"
              onClick={() => void loadState()}
              disabled={busy}
            >
              <RefreshCw size={15} />
              刷新状态
            </button>
            {state && !state.officialLoggedIn && selectedId === "official" && (
              <button
                className="ghost-btn"
                onClick={loginOfficial}
                disabled={busy}
              >
                <LogIn size={15} />
                登录 OpenAI
              </button>
            )}
          </div>
        </section>

        {error && (
          <div className="error-banner">
            <AlertTriangle size={17} />
            <div>
              <strong>操作未完成</strong>
              <span>{error}</span>
            </div>
            <button onClick={() => setError(null)}>
              <X size={15} />
            </button>
          </div>
        )}

        <div className="grid">
          <section className="panel channels-panel">
            <div className="panel-head">
              <div>
                <h2>目标渠道与模型</h2>
                <p>所有状态来自本机真实配置和服务端能力</p>
              </div>
              <button className="add-btn" onClick={() => editChannel()}>
                <Plus size={15} />
                添加渠道
              </button>
            </div>
            <div className="channel-list">
              {allChannels.map((channel) => (
                <div
                  className={`channel-card ${channel.id === selectedId ? "selected" : ""}`}
                  key={channel.id}
                  onClick={() => setSelectedId(channel.id)}
                >
                  <div
                    className={`channel-icon ${channel.id === "official" ? "official" : "custom"}`}
                  >
                    {channel.id === "official" ? (
                      <Sparkles size={18} />
                    ) : (
                      <Plus size={18} />
                    )}
                  </div>
                  <div className="channel-info">
                    <strong>
                      {channel.name}
                      {activeId === channel.id && <small>当前</small>}
                    </strong>
                    <span>
                      {channel.id === "official"
                        ? state?.officialLoggedIn
                          ? "ChatGPT 官方账号已登录"
                          : "需要登录 ChatGPT 官方账号"
                        : `${channel.baseUrl} · ${channel.hasSecret ? "密钥已保存" : "缺少密钥"}`}
                    </span>
                  </div>
                  <div className="channel-right">
                    {channel.id === selectedId && (
                      <button
                        className="activate-channel-btn"
                        disabled={!canSwitch}
                        onClick={(event) => {
                          event.stopPropagation();
                          void openSwitchConfirmation();
                        }}
                      >
                        <Wifi size={13} />
                        {isCurrent ? "重新应用" : "启用"}
                      </button>
                    )}
                    {channel.id !== "official" && (
                      <button
                        className="mini-btn"
                        title="编辑"
                        aria-label={`编辑 ${channel.name}`}
                        onClick={(event) => {
                          event.stopPropagation();
                          editChannel(channel);
                        }}
                      >
                        <Pencil size={13} />
                      </button>
                    )}
                    {!channel.isBuiltIn && channel.id !== "official" && (
                      <button
                        className="mini-btn danger"
                        title="删除"
                        aria-label={`删除 ${channel.name}`}
                        onClick={(event) => {
                          event.stopPropagation();
                          setPendingDelete(channel);
                        }}
                      >
                        <Trash2 size={13} />
                      </button>
                    )}
                    {channel.id !== selectedId && (
                      <ChevronRight size={16} className="muted" />
                    )}
                  </div>
                </div>
              ))}
            </div>
            <div className="model-picker">
              <div>
                <strong>目标模型</strong>
                <span>
                  {modelsLoading
                    ? "正在读取服务端模型…"
                    : (modelError ??
                      modelNotice ??
                      `${models.length} 个可用模型`)}
                </span>
              </div>
              <select
                value={selectedModel}
                onChange={(event) => setSelectedModel(event.target.value)}
                disabled={modelsLoading || !!modelError}
              >
                {models.map((model) => (
                  <option value={model.id} key={model.id}>
                    {model.displayName || model.id}
                    {model.isDefault ? "（默认）" : ""}
                  </option>
                ))}
                {!models.length && selectedModel && (
                  <option value={selectedModel}>{selectedModel}</option>
                )}
              </select>
              <button
                className="icon-btn"
                title="刷新模型"
                onClick={() =>
                  void loadModels(
                    selectedId,
                    selectedModel,
                    selectedId === "official" || selected.validatesModelList,
                  )
                }
              >
                <RefreshCw size={15} className={modelsLoading ? "spin" : ""} />
              </button>
            </div>
            <div className="reasoning-picker">
              <div>
                <strong>推理强度</strong>
                <span>平衡模式能明显缩短多数任务的等待时间</span>
              </div>
              <select
                value={selectedReasoningEffort}
                onChange={(event) =>
                  setSelectedReasoningEffort(event.target.value)
                }
              >
                {availableReasoningEfforts.map((effort) => (
                  <option value={effort} key={effort}>
                    {reasoningLabels[effort] ?? effort}
                  </option>
                ))}
              </select>
              {["high", "xhigh", "max"].includes(selectedReasoningEffort) && (
                <small>
                  <AlertTriangle size={12} />
                  深度推理耗时更长，长任务也更容易经历断线重试
                </small>
              )}
            </div>
            <div className="task-scope-picker">
              <div>
                <strong>{isCurrent ? "更新旧任务范围" : "跨渠道任务处理"}</strong>
                <span>
                  {isCurrent
                    ? "选择需要使用当前 Provider 的旧任务"
                    : "智能续接保留旧任务，并为目标渠道创建精简任务"}
                </span>
              </div>
              {!isCurrent && (
                <div className="scope-options">
                  {(["smart", "switchOnly", "migrate"] as CrossChannelMode[]).map(
                    (mode) => (
                      <button
                        className={crossChannelMode === mode ? "active" : ""}
                        aria-pressed={crossChannelMode === mode}
                        key={mode}
                        onClick={() => {
                          setCrossChannelMode(mode);
                          setSwitchThreadHealth(null);
                        }}
                      >
                        {crossChannelModeLabels[mode]}
                      </button>
                    ),
                  )}
                </div>
              )}
              {(isCurrent || crossChannelMode === "migrate") && (
                <div className="scope-options">
                  {(["recent5", "all", "single"] as SessionScope[]).map((scope) => (
                    <button
                      className={
                        effectiveSessionScope === scope ? "active" : ""
                      }
                      aria-pressed={effectiveSessionScope === scope}
                      key={scope}
                      onClick={() => {
                        setSessionScope(scope);
                        setSwitchThreadHealth(null);
                      }}
                    >
                      {sessionScopeLabels[scope]}
                    </button>
                  ))}
                </div>
              )}
              {requiresSwitchThread && (
                <label>
                  <span>{crossChannelMode === "smart" && !isCurrent ? "续接任务" : "会话 ID"}</span>
                  <input
                    value={threadId}
                    onChange={(event) => {
                      setThreadId(event.target.value);
                      setSwitchThreadHealth(null);
                    }}
                    placeholder="例如 01a0347b-beff-7c60-ae6b-d6cdf766e863"
                    list="modelay-local-thread-ids"
                    spellCheck={false}
                  />
                  <datalist id="modelay-local-thread-ids">
                    {threadOptions.map((thread) => (
                      <option value={thread.threadId} key={thread.threadId}>
                        {thread.title || thread.providerId}
                      </option>
                    ))}
                  </datalist>
                </label>
              )}
              {!isCurrent && (
                <small className="compatibility-note">
                  {crossChannelMode === "smart" ? (
                    <ShieldCheck size={12} />
                  ) : (
                    <AlertTriangle size={12} />
                  )}
                  {crossChannelMode === "smart"
                    ? "旧任务保持不变；目标渠道获得精简续接任务"
                    : crossChannelMode === "switchOnly"
                      ? "只修改全局渠道，新任务使用目标 Provider"
                      : "直接迁移会重放历史，长上下文可能显著变慢"}
                </small>
              )}
              {switchThreadHealth && (
                <small className="compatibility-note">
                  <Activity size={12} />
                  最近输入 {formatTokens(switchThreadHealth.latestInputTokens)} · {switchThreadHealth.riskLabel}
                </small>
              )}
            </div>
          </section>

          <section className="side-stack">
            <div className="panel quota-panel">
              <div className="panel-head compact">
                <div>
                  <h2>真实额度</h2>
                  <p>{active?.name ?? "当前渠道"} · 每 10 秒更新</p>
                </div>
                <button
                  className="icon-btn"
                  aria-label="刷新额度"
                  onClick={() => void refreshUsage()}
                >
                  <RefreshCw size={15} className={usageLoading ? "spin" : ""} />
                </button>
              </div>
              <UsageView usage={usage} error={usageError} />
              <label className="widget-mode">
                <span>额度胶囊</span>
                <select
                  value={state?.dockMode ?? "off"}
                  onChange={(event) =>
                    void setWidgetMode(
                      event.target.value as AppState["dockMode"],
                    )
                  }
                >
                  <option value="free">自由悬浮</option>
                  <option value="edge">靠边隐藏</option>
                  <option value="off">关闭悬浮</option>
                </select>
              </label>
            </div>
            <div className="panel checks-panel">
              <div className="panel-head compact">
                <div>
                  <h2>当前状态</h2>
                  <p>配置与凭据实时检查</p>
                </div>
                <ShieldCheck
                  size={20}
                  className={state?.configConformant ? "green" : "orange"}
                />
              </div>
              <StatusRow
                ok={!!state?.configExists}
                title="配置文件"
                detail={state?.configExists ? "已读取" : "尚未创建"}
              />
              <StatusRow
                ok={!!state?.configConformant}
                title="Provider 配置"
                detail={state?.configConformant ? "一致" : "需要重新切换修复"}
              />
              <StatusRow
                ok={
                  selectedId === "official"
                    ? !!state?.officialLoggedIn
                    : !!selected.hasSecret
                }
                title={selectedId === "official" ? "官方登录" : "渠道密钥"}
                detail={
                  selectedId === "official"
                    ? state?.officialLoggedIn
                      ? "ChatGPT"
                      : "未登录"
                    : selected.hasSecret
                      ? "系统凭据库"
                      : "未保存"
                }
              />
            </div>
          </section>
        </div>

        {report && (
          <section className="panel report-panel">
            <div className="panel-head">
              <div>
                <h2>最近一次切换报告</h2>
                <p>
                  {report.providerId} · {report.model} ·{" "}
                  {reasoningLabels[report.reasoningEffort] ??
                    report.reasoningEffort}{" "}
                  · {sessionScopeLabels[report.sessionScope]} · $
                  {report.imageSkill}
                </p>
              </div>
              <span className="report-ok">
                <Check size={15} />
                已完成
              </span>
            </div>
            <div className="report-grid">
              {report.checks.map((check, index) => (
                <div
                  className={`report-row ${check.state}`}
                  key={`${check.title}-${index}`}
                >
                  <span>
                    {check.state === "passed" ? (
                      <Check size={14} />
                    ) : (
                      <AlertTriangle size={14} />
                    )}
                  </span>
                  <div>
                    <strong>{check.title}</strong>
                    <small>{check.detail}</small>
                  </div>
                </div>
              ))}
            </div>
          </section>
        )}

        <section className="panel handoff-panel">
          <div className="handoff-workflow">
            <div className="panel-head">
              <div>
                <h2>
                  会话续接助手 <span className="beta-badge">内测</span>
                </h2>
                <p>
                  检查上下文风险，并将当天项目资料、最新需求与进度交接到全新任务
                </p>
              </div>
              <Activity
                size={20}
                className={
                  threadHealth?.riskLevel === "critical"
                    ? "risk-critical"
                    : threadHealth?.riskLevel === "warning"
                      ? "orange"
                      : "green"
                }
              />
            </div>
            <div className="handoff-beta-note" role="note">
              <AlertTriangle size={16} />
              <span>
                <strong>内测功能 · 还在测试中尚不稳定</strong>
                续接会读取当天会话摘要并创建新任务；如果失败，旧任务不会被修改。建议先保存工作区，再进行测试。
              </span>
            </div>
            <div className="local-thread-picker">
              <div className="local-thread-picker-head">
                <label htmlFor="local-thread-select">从本地旧任务选择</label>
                <button
                  className="mini-btn"
                  aria-label="刷新本地任务列表"
                  title="刷新本地任务列表"
                  onClick={() => void loadThreads()}
                  disabled={threadsLoading}
                >
                  <RefreshCw
                    size={14}
                    className={threadsLoading ? "spin" : ""}
                  />
                </button>
              </div>
              <select
                id="local-thread-select"
                value={
                  threadOptions.some(
                    (thread) => thread.threadId === handoffThreadId,
                  )
                    ? handoffThreadId
                    : ""
                }
                onChange={(event) => {
                  setHandoffThreadId(event.target.value);
                  setThreadHealth(null);
                  setHandoffReport(null);
                  setHandoffError(null);
                }}
                disabled={threadsLoading}
              >
                <option value="">
                  {threadsLoading
                    ? "正在读取本地任务…"
                    : `请选择任务（最近 ${threadOptions.length} 个）`}
                </option>
                {threadOptions.map((thread) => (
                  <option value={thread.threadId} key={thread.threadId}>
                    {thread.issue ? "需修复 · " : "可检查 · "}
                    {thread.title || thread.threadId} · {thread.providerId} / {thread.model}
                  </option>
                ))}
              </select>
              <small>
                直接读取本机 Codex 任务索引，不需要从 ChatGPT 复制会话 ID。
              </small>
              {threadListError && (
                <div className="thread-diagnostic bad" role="alert">
                  <AlertTriangle size={14} />
                  <span>{threadListError}</span>
                </div>
              )}
              {selectedThread && (
                <div
                  className={`thread-diagnostic ${selectedThread.issue ? "bad" : "good"}`}
                  role="status"
                >
                  {selectedThread.issue ? (
                    <AlertTriangle size={14} />
                  ) : (
                    <ShieldCheck size={14} />
                  )}
                  <div>
                    <strong>
                      {selectedThread.issue
                        ? "发现旧任务兼容问题"
                        : "任务索引与渠道配置可解析"}
                    </strong>
                    <span>
                      {selectedThread.issue ||
                        `${selectedThread.providerId} · ${selectedThread.model} · ${formatThreadDate(selectedThread.updatedAtMs)}`}
                    </span>
                    <div className="thread-id-line">
                      <code>{selectedThread.threadId}</code>
                      <button
                        className="mini-btn"
                        aria-label="复制所选任务 ID"
                        title="复制所选任务 ID"
                        onClick={() =>
                          void copyText(selectedThread.threadId, "任务 ID")
                        }
                      >
                        <Copy size={13} />
                      </button>
                    </div>
                  </div>
                </div>
              )}
            </div>
            <div className="handoff-input">
              <label htmlFor="handoff-thread-id">或手动输入会话 ID</label>
              <div>
                <input
                  id="handoff-thread-id"
                  value={handoffThreadId}
                  onChange={(event) => {
                    setHandoffThreadId(event.target.value);
                    setThreadHealth(null);
                    setHandoffReport(null);
                    setHandoffError(null);
                  }}
                  placeholder="例如 01a041f4-ef73-7560-b418-c930ab8b6af0"
                  spellCheck={false}
                />
                <button
                  className="ghost-btn"
                  onClick={() => void inspectThread()}
                  disabled={handoffBusy}
                >
                  {handoffBusy && !threadHealth ? (
                    <RefreshCw size={14} className="spin" />
                  ) : (
                    <Activity size={14} />
                  )}
                  检查会话
                </button>
              </div>
            </div>
            {handoffError && (
              <div className="handoff-error" role="alert">
                <AlertTriangle size={17} />
                <div>
                  <strong>续接任务没有创建完成</strong>
                  <span>{handoffError}</span>
                  <small>
                    请确认当前渠道已经启用并重启
                    Codex，然后重新检查该会话再试一次。旧任务不会被修改。
                  </small>
                </div>
              </div>
            )}
            {threadHealth && (
              <div
                className={`thread-health ${threadHealth.riskLevel}`}
                role="status"
              >
                <div className="health-head">
                  <span>{threadHealth.riskLabel}</span>
                  <strong>
                    {formatTokens(threadHealth.tokensUsed)} tokens
                  </strong>
                </div>
                <h3>{threadHealth.title || threadHealth.threadId}</h3>
                <p>{threadHealth.cwd}</p>
                <div className="health-stats">
                  <span>
                    当天消息 <b>{threadHealth.todayMessageCount}</b>
                  </span>
                  <span>
                    记录体积{" "}
                    <b>{formatBytes(threadHealth.todayRolloutBytes)}</b>
                  </span>
                  <span>
                    最近输入 <b>{formatTokens(threadHealth.latestInputTokens)}</b>
                  </span>
                  <span>
                    {threadHealth.providerId} · {threadHealth.model} ·{" "}
                    {reasoningLabels[threadHealth.reasoningEffort] ??
                      threadHealth.reasoningEffort}
                  </span>
                </div>
                <ul>
                  {threadHealth.riskReasons.map((reason) => (
                    <li key={reason}>{reason}</li>
                  ))}
                </ul>
                {threadHealth.latestUserRequest && (
                  <div className="latest-request">
                    <span>最新需求</span>
                    <p>{threadHealth.latestUserRequest}</p>
                  </div>
                )}
                <div className="handoff-actions">
                  <button
                    className="ghost-btn"
                    onClick={() => void compactThread()}
                    disabled={handoffBusy}
                  >
                    {handoffBusy ? <RefreshCw size={15} className="spin" /> : <RefreshCw size={15} />}
                    压缩原任务
                  </button>
                  <button
                    className="primary-btn handoff-action"
                    onClick={() => void createHandoff()}
                    disabled={handoffBusy}
                  >
                    {handoffBusy ? (
                      <RefreshCw size={15} className="spin" />
                    ) : (
                      <ArrowRight size={15} />
                    )}
                    整理并创建续接任务
                  </button>
                </div>
              </div>
            )}
            {handoffReport && (
              <div className="handoff-success" role="status">
                <Check size={18} />
                <div>
                  <strong>新任务与交接摘要已创建</strong>
                  <span>{handoffReport.newThreadId}</span>
                  <small>
                    已提取当天 {handoffReport.messageCount} 条消息和{" "}
                    {handoffReport.referencedPaths.length}{" "}
                    个引用路径；不会后台运行模型或占用新会话。若 Codex
                    任务列表尚未刷新，可重启 Codex 后直接打开。
                  </small>
                </div>
                <button
                  className="mini-btn"
                  aria-label="复制新任务 ID"
                  title="复制新任务 ID"
                  onClick={() =>
                    void copyText(handoffReport.newThreadId, "新任务 ID")
                  }
                >
                  <Copy size={14} />
                </button>
              </div>
            )}
          </div>
          <aside className="handoff-guide">
            <div className="guide-title">
              <FileText size={17} />
              <div>
                <strong>什么时候需要续接？</strong>
                <span>以下情况可能表现为长时间思考、重新连接或请求失败</span>
              </div>
            </div>
            <div className="guide-list">
              <div>
                <b>上下文累计过大</b>
                <span>
                  长期项目、频繁工具调用、图片与大型输出会增加恢复和压缩成本。
                </span>
              </div>
              <div>
                <b>单次请求载荷过大</b>
                <span>
                  历史与新消息合并后可能触发 413、超时或服务端主动断开。
                </span>
              </div>
              <div>
                <b>渠道或网络不稳定</b>
                <span>
                  第三方首字节慢、限流、模型排队或重启后凭据未生效都会触发重试。
                </span>
              </div>
              <div>
                <b>模型深度过高</b>
                <span>
                  高、极深或最大推理会显著延长首个响应时间，长任务更容易碰到连接窗口。
                </span>
              </div>
            </div>
            <div className="guide-solution">
              <strong>推荐解决方案</strong>
              <span>
                先检查会话；风险较高时创建续接任务。Modelay
                只传递当天精简交接，不复制完整历史，并要求新任务先核对工作区和现有文件。
              </span>
            </div>
          </aside>
        </section>

        <section className="panel bottom-panel">
          <div className="bottom-item">
            <div className="bottom-icon">
              <KeyRound size={17} />
            </div>
            <div>
              <strong>系统安全存储</strong>
              <span>API Key 不写入 config.toml 或前端存储</span>
            </div>
          </div>
          <div className="bottom-item">
            <div className="bottom-icon">
              <Database size={17} />
            </div>
            <div>
              <strong>配置与任务双备份</strong>
              <span>SQLite 事务只覆盖用户任务</span>
            </div>
          </div>
          <div className="bottom-item">
            <div className="bottom-icon">
              <Copy size={17} />
            </div>
            <div>
              <strong>生图路由</strong>
              <span>当前默认 ${state?.imageSkill ?? "读取中"}</span>
            </div>
          </div>
        </section>
        <div className="statusbar">
          <span>
            <span className={`dot ${error ? "warn" : ""}`} />
            {message}
          </span>
        </div>
      </main>

      {draft && (
        <div className="modal-backdrop">
          <div className="modal">
            <div className="modal-head">
              <div>
                <h2>
                  {state?.channels.some((channel) => channel.id === draft.id)
                    ? `编辑 ${draft.name}`
                    : "添加自定义渠道"}
                </h2>
                <p>Codex 自定义 Provider 固定使用 Responses API</p>
              </div>
              <button className="icon-btn" onClick={() => setDraft(null)}>
                <X size={18} />
              </button>
            </div>
            <label>
              渠道名称
              <input
                value={draft.name}
                onChange={(event) =>
                  setDraft({ ...draft, name: event.target.value })
                }
              />
            </label>
            <label>
              API 地址
              <input
                value={draft.baseUrl}
                onChange={(event) =>
                  setDraft({ ...draft, baseUrl: event.target.value })
                }
                placeholder="https://example.com"
              />
            </label>
            <label>
              API 密钥
              <input
                type="password"
                value={draft.secret}
                onChange={(event) =>
                  setDraft({ ...draft, secret: event.target.value })
                }
                placeholder={
                  draft.hasSecret ? "留空则保留已保存密钥" : "请输入密钥"
                }
                autoComplete="new-password"
              />
            </label>
            <div className="form-row">
              <label>
                默认模型
                <select
                  value={draft.model}
                  onChange={(event) =>
                    setDraft({ ...draft, model: event.target.value })
                  }
                >
                  {(models.length ? models : commonChannelModels).map((model) => (
                    <option value={model.id} key={model.id}>
                      {model.displayName || model.id}
                    </option>
                  ))}
                  {draft.model &&
                    !(models.length ? models : commonChannelModels).some(
                      (model) => model.id === draft.model,
                    ) && <option value={draft.model}>{draft.model}</option>}
                </select>
              </label>
              <label>
                默认推理强度
                <select
                  value={draft.reasoningEffort}
                  onChange={(event) =>
                    setDraft({ ...draft, reasoningEffort: event.target.value })
                  }
                >
                  {Object.entries(reasoningLabels).map(([value, label]) => (
                    <option value={value} key={value}>
                      {label}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <div className="form-row">
              <label>
                模型列表路径
                <input
                  value={draft.modelsPath}
                  onChange={(event) =>
                    setDraft({ ...draft, modelsPath: event.target.value })
                  }
                />
              </label>
              <label>
                余额路径
                <input value={draft.usagePath || "/v1/usage"} readOnly aria-describedby="usage-path-help" />
                <small id="usage-path-help" className="field-help">默认自动使用 /v1/usage；查询失败时 Modelay 会尝试常见余额接口。</small>
              </label>
            </div>
            <label className="toggle-row">
              <input
                type="checkbox"
                checked={draft.validatesModelList}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    validatesModelList: event.target.checked,
                  })
                }
              />
              切换前校验服务端模型列表
            </label>
            <div className="secret-note">
              <KeyRound size={15} />
              <span>
                {draft.hasSecret
                  ? "已有密钥保存在系统凭据库。输入新值会覆盖，留空保持不变。"
                  : "密钥将写入 macOS Keychain 或 Windows Credential Manager，不会返回给界面。"}
              </span>
            </div>
            <div className="modal-actions">
              {draft.hasSecret && (
                <button
                  className="danger-btn"
                  onClick={() => setConfirmSecretDelete(true)}
                >
                  删除密钥
                </button>
              )}
              <button className="ghost-btn" onClick={() => setDraft(null)}>
                取消
              </button>
              <button
                className="primary-btn"
                onClick={() => void saveChannel()}
                disabled={busy}
              >
                保存渠道
              </button>
            </div>
          </div>
        </div>
      )}

      {infoPanel && (
        <div className="modal-backdrop">
          <div className="modal info-modal">
            <div className="modal-head">
              <div>
                <h2>
                  {infoPanel === "help" ? "Modelay 使用帮助" : "Modelay 设置"}
                </h2>
                <p>
                  {infoPanel === "help"
                    ? "渠道切换、旧任务覆盖与安全边界"
                    : "当前运行环境、本地数据与软件更新"}
                </p>
              </div>
              <button className="icon-btn" onClick={() => setInfoPanel(null)}>
                <X size={18} />
              </button>
            </div>
            {infoPanel === "help" ? (
              <div className="info-list">
                <div>
                  <strong>切换渠道</strong>
                  <span>
                    选择渠道后，在渠道卡片上点击“启用”。Modelay
                    会先展示确认信息，再备份、写配置和验证服务。
                  </span>
                </div>
                <div>
                  <strong>继续旧任务</strong>
                  <span>
                    可选择最近 5 个、全部旧任务或输入一个会话
                    ID；只修改匹配用户任务的
                    Provider、模型和推理强度，不修改消息或 rollout。
                  </span>
                </div>
                <div>
                  <strong>生图路由</strong>
                  <span>
                    官方渠道使用 $imagegen；自定义第三方渠道使用 $imagegen2。
                  </span>
                </div>
                <div>
                  <strong>软件更新</strong>
                  <span>
                    启动后自动检查签名更新，也可以在设置中手动检查；安装前不会打断当前任务。
                  </span>
                </div>
                <div>
                  <strong>出现错误</strong>
                  <span>
                    切换事务会恢复配置、环境变量、偏好和生图路由；可从备份目录检查原始文件。
                  </span>
                </div>
              </div>
            ) : (
              <div className="info-list">
                <div>
                  <strong>当前 Provider</strong>
                  <span>
                    {state?.currentProviderId ?? "读取中"} ·{" "}
                    {state?.currentModel || "未设置模型"}
                  </span>
                </div>
                <div>
                  <strong>应用平台</strong>
                  <span>{state?.platform ?? "读取中"}</span>
                </div>
                <div>
                  <strong>备份目录</strong>
                  <span className="path-text">
                    {state?.backupDirectory ?? "读取中"}
                  </span>
                </div>
                <div>
                  <strong>安全存储</strong>
                  <span>
                    {state?.platform.startsWith("windows")
                      ? "Windows Credential Manager"
                      : "macOS Keychain"}
                  </span>
                </div>
                <div className="update-setting">
                  <strong>软件更新 · {appVersion}</strong>
                  <span>{updateMessage}</span>
                  <div className="release-overview">
                    <section>
                      <b>版本说明</b>
                      <p>{currentReleaseInfo.summary}</p>
                    </section>
                    <section>
                  <b>本次更新内容</b>
                  <ul>
                    {currentReleaseInfo.changes
                      .slice(0, showAllChanges ? undefined : 4)
                      .map((item) => (
                      <li key={item}>{item}</li>
                      ))}
                  </ul>
                  {currentReleaseInfo.changes.length > 4 && (
                    <button className="ghost-btn info-action" onClick={() => setShowAllChanges((value) => !value)}>
                      {showAllChanges ? "收起更新内容" : "查看全部更新内容"}
                    </button>
                  )}
                    </section>
                  </div>
                  {updatePhase === "downloading" ||
                  updatePhase === "installing" ? (
                    <div className="update-progress">
                      <i style={{ width: `${updateProgress ?? 12}%` }} />
                    </div>
                  ) : null}
                  <button
                    className="ghost-btn info-action"
                    onClick={() =>
                      void (updatePhase === "available"
                        ? setUpdateOpen(true)
                        : checkForUpdates(true))
                    }
                    disabled={
                      updatePhase === "checking" ||
                      updatePhase === "downloading" ||
                      updatePhase === "installing"
                    }
                  >
                    {updatePhase === "checking" ? (
                      <RefreshCw size={15} className="spin" />
                    ) : updatePhase === "available" ? (
                      <Download size={15} />
                    ) : (
                      <RefreshCw size={15} />
                    )}
                    {updatePhase === "available"
                      ? `安装 ${updateVersion}`
                      : "检查更新"}
                  </button>
                </div>
                <button
                  className="ghost-btn info-action"
                  onClick={() => void invoke("open_backup_folder")}
                >
                  <FolderOpen size={15} />
                  打开备份目录
                </button>
              </div>
            )}
          </div>
        </div>
      )}

      {pendingDelete && (
        <div className="modal-backdrop">
          <div className="modal confirm-modal">
            <div className="restart-icon danger-icon">
              <Trash2 size={21} />
            </div>
            <h2>删除 {pendingDelete.name}？</h2>
            <p>将删除渠道资料、系统凭据和残留环境变量。备份文件不会被删除。</p>
            <div className="modal-actions">
              <button
                className="ghost-btn"
                onClick={() => setPendingDelete(null)}
              >
                取消
              </button>
              <button
                className="danger-btn"
                onClick={() => void deleteChannel(pendingDelete)}
                disabled={busy}
              >
                确认删除
              </button>
            </div>
          </div>
        </div>
      )}

      {confirmSecretDelete && draft && (
        <div className="modal-backdrop nested-modal">
          <div className="modal confirm-modal">
            <div className="restart-icon danger-icon">
              <KeyRound size={21} />
            </div>
            <h2>删除 {draft.name} 的密钥？</h2>
            <p>
              删除后该渠道将无法查询模型、余额或执行切换，直到重新保存密钥。
            </p>
            <div className="modal-actions">
              <button
                className="ghost-btn"
                onClick={() => setConfirmSecretDelete(false)}
              >
                取消
              </button>
              <button
                className="danger-btn"
                onClick={() => void deleteSecret()}
                disabled={busy}
              >
                确认删除密钥
              </button>
            </div>
          </div>
        </div>
      )}

      {switchConfirmOpen && (
        <div className="modal-backdrop">
          <div className="modal switch-confirm-modal">
            <div className="restart-icon">
              <Wifi size={21} />
            </div>
            <h2>
              {isCurrent
                ? `重新应用 ${selected.name}？`
                : `启用 ${selected.name}？`}
            </h2>
            <p>
              {isCurrent
                ? "确认后将重新应用全局配置，并更新所选范围内的旧任务。"
                : crossChannelMode === "smart"
                  ? "确认后将切换全局配置，并在目标渠道创建精简续接任务。"
                  : crossChannelMode === "switchOnly"
                    ? "确认后只切换全局配置，不改写任何旧任务。"
                    : "确认后将切换全局配置，并直接改写所选旧任务的 Provider。"}
            </p>
            <div className="switch-summary">
              <div>
                <span>目标模型</span>
                <strong>{selectedModel}</strong>
              </div>
              <div>
                <span>推理强度</span>
                <strong>
                  {reasoningLabels[selectedReasoningEffort] ??
                    selectedReasoningEffort}
                </strong>
              </div>
              <div>
                <span>{isCurrent ? "旧任务范围" : "任务处理"}</span>
                <strong>
                  {!isCurrent && crossChannelMode !== "migrate"
                    ? crossChannelModeLabels[crossChannelMode]
                    : effectiveSessionScope === "single"
                      ? threadId.trim()
                      : sessionScopeLabels[effectiveSessionScope]}
                </strong>
              </div>
            </div>
            {switchThreadHealth && (
              <div
                className={`thread-diagnostic ${switchThreadHealth.riskLevel === "healthy" ? "good" : "bad"}`}
                role="status"
              >
                {switchThreadHealth.riskLevel === "healthy" ? (
                  <ShieldCheck size={14} />
                ) : (
                  <AlertTriangle size={14} />
                )}
                <div>
                  <strong>{switchThreadHealth.riskLabel}</strong>
                  <span>
                    最近一轮约 {formatTokens(switchThreadHealth.latestInputTokens)} 输入 tokens
                  </span>
                </div>
              </div>
            )}
            <div className="modal-actions">
              <button
                className="ghost-btn"
                onClick={() => setSwitchConfirmOpen(false)}
              >
                取消
              </button>
              <button
                className="primary-btn"
                onClick={() => void switchChannel()}
                disabled={!canSwitch}
              >
                {busy ? (
                  <RefreshCw size={15} className="spin" />
                ) : (
                  <Wifi size={15} />
                )}
                确认启用
              </button>
            </div>
          </div>
        </div>
      )}

      {switchInProgress && (
        <div className="modal-backdrop operation-backdrop">
          <div
            className="modal operation-modal"
            role="status"
            aria-live="polite"
          >
            <div className="operation-spinner">
              <RefreshCw size={24} className="spin" />
            </div>
            <h2>正在切换到 {selected.name}</h2>
            <p>
              Modelay
              正在依次备份配置、验证模型与服务、更新任务索引。完成前渠道尚未完全生效，请不要关闭软件。
            </p>
            <div className="operation-steps">
              <span>
                <Check size={13} />
                已确认目标渠道与模型
              </span>
              <span>
                <RefreshCw size={13} className="spin" />
                正在写入、验证并生成回滚备份
              </span>
              <span>完成后将立即显示“重启 Codex”确认</span>
            </div>
          </div>
        </div>
      )}

      {restartOpen && (
        <div className="modal-backdrop">
          <div className="modal restart-modal">
            <div className="restart-icon">
              <RefreshCw size={22} />
            </div>
            <h2>渠道已成功启用</h2>
            <p>
              {switchHandoffReport
                ? "目标渠道和智能续接任务已经准备完成。重启后可在任务列表中继续。"
                : "配置、诊断和所选任务更新已经完成。需要重启 ChatGPT/Codex 才能让新渠道完整生效；立即重启会中断仍在运行的任务。"}
            </p>
            {switchHandoffReport && (
              <div className="thread-diagnostic good" role="status">
                <ShieldCheck size={14} />
                <div>
                  <strong>智能续接任务已创建</strong>
                  <div className="thread-id-line">
                    <code>{switchHandoffReport.newThreadId}</code>
                    <button
                      className="mini-btn"
                      aria-label="复制续接任务 ID"
                      title="复制续接任务 ID"
                      onClick={() =>
                        void copyText(switchHandoffReport.newThreadId, "续接任务 ID")
                      }
                    >
                      <Copy size={13} />
                    </button>
                  </div>
                </div>
              </div>
            )}
            <div className="modal-actions">
              <button
                className="ghost-btn"
                onClick={() => {
                  setRestartOpen(false);
                  setMessage("配置已生效，请稍后手动重启 ChatGPT/Codex");
                }}
              >
                稍后手动重启
              </button>
              <button className="primary-btn" onClick={() => void restartNow()}>
                立即重启 Codex
              </button>
            </div>
          </div>
        </div>
      )}
      {updateOpen && updateVersion && (
        <div className="modal-backdrop">
          <div className="modal update-modal">
            <div className="restart-icon">
              <Download size={22} />
            </div>
            <h2>Modelay {updateVersion} 可用</h2>
            <p>更新包会先验证 Modelay 的数字签名，验证失败将拒绝安装。</p>
            <div className="update-release-details">
              <section>
                <strong>版本说明</strong>
                <span>
                  当前版本 {appVersion}，可更新至 {updateVersion}。
                </span>
              </section>
              <section>
                <strong>本次更新内容</strong>
                <div className="release-notes">
                  {updateNotes || "这个版本暂时没有附带更新说明。"}
                </div>
              </section>
            </div>
            {updatePhase === "downloading" || updatePhase === "installing" ? (
              <div className="modal-update-progress">
                <div>
                  <i style={{ width: `${updateProgress ?? 12}%` }} />
                </div>
                <span>
                  {updateMessage}
                  {updateProgress !== null ? ` · ${updateProgress}%` : ""}
                </span>
              </div>
            ) : (
              <div className="update-warning">
                <AlertTriangle size={15} />
                <span>
                  安装完成后 Modelay 会自动重启，请先结束正在执行的重要操作。
                </span>
              </div>
            )}
            <div className="modal-actions">
              <button
                className="ghost-btn"
                onClick={() => setUpdateOpen(false)}
                disabled={
                  updatePhase === "downloading" || updatePhase === "installing"
                }
              >
                稍后提醒
              </button>
              <button
                className="primary-btn"
                onClick={() => void installUpdate()}
                disabled={
                  updatePhase === "downloading" || updatePhase === "installing"
                }
              >
                {updatePhase === "downloading" ||
                updatePhase === "installing" ? (
                  <RefreshCw size={15} className="spin" />
                ) : (
                  <Download size={15} />
                )}
                立即更新并重启
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function StatusRow({
  ok,
  title,
  detail,
}: {
  ok: boolean;
  title: string;
  detail: string;
}) {
  return (
    <div className={`check-row ${ok ? "" : "bad"}`}>
      {ok ? <Check size={15} /> : <AlertTriangle size={15} />}
      <span>{title}</span>
      <em>{detail}</em>
    </div>
  );
}

function UsageView({
  usage,
  error,
}: {
  usage: UsageSnapshot | null;
  error: string | null;
}) {
  if (!usage && error)
    return (
      <div className="usage-empty">
        <AlertTriangle size={16} />
        <span>{error}</span>
      </div>
    );
  if (!usage)
    return (
      <div className="usage-empty">
        <RefreshCw size={16} className="spin" />
        <span>正在读取额度…</span>
      </div>
    );
  if (usage.kind === "official")
    return (
      <div className="real-usage">
        <Quota
          label={quotaLabel(usage.fiveHour, "short")}
          window={usage.fiveHour}
        />
        <Quota
          label={quotaLabel(usage.weekly, "weekly")}
          window={usage.weekly}
        />
        {usage.creditsBalance && (
          <div className="credit-line">
            可用 Credits：{usage.creditsBalance}
          </div>
        )}
        {error && (
          <small className="usage-warning">
            更新失败，保留上次数据：{error}
          </small>
        )}
      </div>
    );
  return (
    <div className="real-usage balance">
      <strong>{usage.balanceLabel ?? "可用余额"}</strong>
      <b>
        {usage.remainingBalance?.toLocaleString(undefined, {
          maximumFractionDigits: 4,
        }) ?? "—"}
      </b>
      <span>{usage.planName ?? "第三方渠道"}</span>
      {error && (
        <small className="usage-warning">更新失败，保留上次数据：{error}</small>
      )}
    </div>
  );
}

function Quota({
  label,
  window: quota,
}: {
  label: string;
  window?: UsageWindow;
}) {
  const reset = quota?.resetsAt
    ? new Date(quota.resetsAt * 1000).toLocaleString()
    : "未知";
  const quotaName = label.endsWith("额度") ? label : `${label}额度`;
  const tooltip = `${quotaName}重置时间：${reset}`;
  return (
    <div
      className="quota-line"
      title={tooltip}
      data-reset={tooltip}
      tabIndex={0}
      aria-label={tooltip}
    >
      <span>{label}</span>
      <div>
        <i style={{ width: `${quota?.remainingPercent ?? 0}%` }} />
      </div>
      <b>{quota ? `${Math.round(quota.remainingPercent)}%` : "—"}</b>
    </div>
  );
}

function formatTokens(value: number) {
  return value >= 1_000_000
    ? `${(value / 1_000_000).toFixed(2)}M`
    : value >= 1_000
      ? `${Math.round(value / 1_000)}K`
      : String(value);
}
function formatBytes(value: number) {
  return value >= 1024 * 1024
    ? `${(value / 1024 / 1024).toFixed(1)} MB`
    : value >= 1024
      ? `${Math.round(value / 1024)} KB`
      : `${value} B`;
}

function formatThreadDate(value: number) {
  if (!value) return "更新时间未知";
  return new Date(value).toLocaleString(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export default App;
