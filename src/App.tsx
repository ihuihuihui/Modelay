import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { AlertTriangle, Check, ChevronRight, CircleHelp, Cloud, Copy, Database, Download, FolderOpen, KeyRound, LogIn, Pencil, Plus, RefreshCw, Settings2, ShieldCheck, Sparkles, Trash2, Wifi, X } from "lucide-react";
import { resolveManualModelFallback } from "./modelSelection";
import { classifyUpdaterError, downloadPercent, type UpdatePhase } from "./updateState";
import { quotaLabel } from "./usageFormatting";

type Channel = {
  id: string; name: string; baseUrl: string; model: string; modelsPath: string;
  usagePath: string; validatesModelList: boolean; isBuiltIn: boolean; hasSecret?: boolean;
};
type AppState = {
  platform: string; currentMode: "official" | "channel" | "unknown"; currentChannelId?: string;
  currentProviderId: string; currentModel: string; officialLoggedIn: boolean; configExists: boolean;
  configConformant: boolean; imageSkill: string; channels: Channel[]; officialModel: string; backupDirectory: string;
  dockMode: "free" | "edge" | "off"; widgetPosition?: { x: number; y: number };
};
type ModelInfo = { id: string; displayName: string; description: string; isDefault: boolean; supportedReasoningEfforts: string[] };
type CheckResult = { title: string; detail: string; state: "passed" | "warning" | "failed" };
type SwitchReport = { channelId: string; providerId: string; model: string; imageSkill: string; backupPath: string; needsRestart: boolean; checks: CheckResult[] };
type UsageWindow = { remainingPercent: number; durationMinutes?: number; resetsAt?: number };
type UsageSnapshot = { kind: "official" | "channel"; channelId: string; planName?: string; fiveHour?: UsageWindow; weekly?: UsageWindow; remainingBalance?: number; balanceLabel?: string; creditsBalance?: string; updatedAt: number };
type Draft = Channel & { secret: string };
type InfoPanel = "help" | "settings" | null;

const officialChannel: Channel = { id: "official", name: "OpenAI 官方", baseUrl: "ChatGPT 账号登录", model: "", modelsPath: "", usagePath: "", validatesModelList: true, isBuiltIn: true };

function App() {
  const [state, setState] = useState<AppState | null>(null);
  const [selectedId, setSelectedId] = useState("official");
  const [selectedModel, setSelectedModel] = useState("");
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
  const pendingUpdate = useRef<Update | null>(null);
  const startupUpdateCheck = useRef(false);

  const allChannels = useMemo(() => [officialChannel, ...(state?.channels ?? [])], [state]);
  const selected = allChannels.find((channel) => channel.id === selectedId) ?? officialChannel;
  const activeId = state?.currentMode === "official" ? "official" : state?.currentChannelId;
  const active = allChannels.find((channel) => channel.id === activeId);

  const loadState = useCallback(async () => {
    try {
      const next = await invoke<AppState>("get_app_state");
      setState(next);
      const actual = next.currentMode === "official" ? "official" : next.currentChannelId ?? "official";
      setSelectedId((current) => current === "official" && actual !== "official" ? actual : current);
      setError(null); setMessage("已读取当前 Codex 配置");
    } catch (reason) { setError(String(reason)); setMessage("无法读取当前配置"); }
  }, []);

  useEffect(() => { void loadState(); }, [loadState]);

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
      setUpdateOpen(true);
    } catch (reason) {
      const classified = classifyUpdaterError(reason);
      setUpdatePhase(classified.phase);
      setUpdateMessage(classified.message);
      if (manual && classified.phase === "error") setError(classified.message);
    }
  }, []);

  useEffect(() => {
    void getVersion().then(setAppVersion).catch(() => undefined);
    if (startupUpdateCheck.current) return;
    startupUpdateCheck.current = true;
    const timer = window.setTimeout(() => void checkForUpdates(false), 4_000);
    return () => window.clearTimeout(timer);
  }, [checkForUpdates]);

  const loadModels = useCallback(async (channelId: string, fallbackModel: string, validatesModelList: boolean) => {
    setModelsLoading(true); setModelError(null); setModelNotice(null); setModels([]);
    try {
      const result = await invoke<ModelInfo[]>("list_models", { channelId });
      const fallback = fallbackModel.trim();
      const manual = !result.length ? resolveManualModelFallback(fallback, validatesModelList) : null;
      const usable = manual ? [manual.model] : result;
      setModels(usable);
      const preferred = fallbackModel || result.find((model) => model.isDefault)?.id || result[0]?.id || "";
      setSelectedModel(usable.some((model) => model.id === preferred) ? preferred : usable[0]?.id ?? "");
      if (manual) setModelNotice(manual.notice);
    } catch (reason) {
      const manual = resolveManualModelFallback(fallbackModel, validatesModelList, String(reason));
      if (manual) {
        setModels([manual.model]);
        setSelectedModel(manual.model.id);
        setModelNotice(manual.notice);
      } else {
        setModelError(String(reason)); setSelectedModel(fallbackModel);
      }
    }
    finally { setModelsLoading(false); }
  }, []);

  useEffect(() => {
    if (!state) return;
    const selectedChannel = state.channels.find((channel) => channel.id === selectedId);
    const fallback = selectedId === "official" ? state.officialModel : selectedChannel?.model ?? "";
    void loadModels(selectedId, fallback, selectedId === "official" || (selectedChannel?.validatesModelList ?? true));
  }, [state, selectedId, loadModels]);

  const refreshUsage = useCallback(async () => {
    if (!activeId) return;
    setUsageLoading(true);
    try { setUsage(await invoke<UsageSnapshot>("get_usage", { channelId: activeId })); setUsageError(null); }
    catch (reason) { setUsageError(String(reason)); }
    finally { setUsageLoading(false); }
  }, [activeId]);

  useEffect(() => {
    void refreshUsage();
    const timer = window.setInterval(() => void refreshUsage(), 10_000);
    return () => window.clearInterval(timer);
  }, [refreshUsage]);

  async function switchChannel() {
    if (!selectedModel || modelError) return;
    setBusy(true); setError(null); setReport(null); setMessage("正在备份、切换并验证真实配置…");
    try {
      const result = await invoke<SwitchReport>("switch_channel", { request: { channelId: selectedId, model: selectedModel } });
      setReport(result); setRestartOpen(result.needsRestart); setMessage(`已切换为 ${selected.name} / ${selectedModel}`);
      await loadState(); await refreshUsage();
    } catch (reason) { setError(String(reason)); setMessage("切换失败，已尝试恢复原配置"); }
    finally { setBusy(false); }
  }

  async function saveChannel() {
    if (!draft) return;
    setBusy(true); setError(null);
    try {
      const channel: Channel = { ...draft }; delete (channel as Partial<Draft>).secret;
      const next = await invoke<AppState>("save_channel", { request: { channel, secret: draft.secret || null } });
      setState(next); setSelectedId(channel.id); setDraft(null); setMessage(`${channel.name} 已安全保存`);
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  }

  async function deleteChannel(channel: Channel) {
    if (channel.isBuiltIn) return;
    setBusy(true);
    try { setState(await invoke<AppState>("delete_channel", { channelId: channel.id })); setSelectedId("official"); setPendingDelete(null); setMessage(`${channel.name} 已删除`); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  }

  async function deleteSecret() {
    if (!draft) return;
    setBusy(true);
    try { setState(await invoke<AppState>("delete_secret", { channelId: draft.id })); setDraft({ ...draft, hasSecret: false, secret: "" }); setConfirmSecretDelete(false); setMessage(`${draft.name} 的密钥已删除`); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  }

  async function loginOfficial() {
    setBusy(true); setMessage("请在浏览器中完成 OpenAI 登录…"); setError(null);
    try { setState(await invoke<AppState>("login_official")); setMessage("OpenAI 官方账号登录有效"); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  }

  async function restartNow() {
    setRestartOpen(false); setBusy(true); setMessage("正在重启 ChatGPT…");
    try { await invoke("restart_chatgpt"); setMessage("ChatGPT 已重新打开"); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
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
      setMessage(mode === "free" ? "额度胶囊已设为自由悬浮" : mode === "edge" ? "额度胶囊已设为靠边隐藏" : "额度胶囊已关闭");
    } catch (reason) { setError(String(reason)); }
  }

  function editChannel(channel?: Channel) {
    const value = channel ?? { id: `channel-${crypto.randomUUID().slice(0, 8)}`, name: "新建渠道", baseUrl: "https://", model: "gpt-5.6-sol", modelsPath: "/v1/models", usagePath: "/v1/usage", validatesModelList: true, isBuiltIn: false };
    setDraft({ ...value, secret: "" });
  }

  const isCurrent = activeId === selectedId;
  const canSwitch = !!state && !!selectedModel && !busy && !modelsLoading && !modelError && (selectedId === "official" ? state.officialLoggedIn : !!selected.hasSecret);

  return <div className="shell">
    <header className="topbar">
      <div className="brand"><div className="brand-mark"><Sparkles size={18} /></div><div><div className="brand-name">Modelay</div><div className="brand-sub">AI 渠道与额度管理器 · 4.0 Alpha</div></div></div>
      <div className="top-actions"><span className="platform"><span className={`dot ${state?.configConformant ? "" : "warn"}`} />{state?.platform ?? "读取中"}</span>{updatePhase === "available" && <button className="update-badge" title={`可更新至 ${updateVersion}`} onClick={() => setUpdateOpen(true)}><Download size={13} />新版本</button>}<button className="icon-btn" title="打开备份目录" onClick={() => void invoke("open_backup_folder")}><FolderOpen size={17} /></button><button className="icon-btn" title="帮助" onClick={() => setInfoPanel("help")}><CircleHelp size={17} /></button><button className="icon-btn" title="设置" onClick={() => setInfoPanel("settings")}><Settings2 size={17} /></button></div>
    </header>
    <main className="content">
      <section className="hero"><div><p className="eyebrow">当前实际渠道</p><h1>{active?.name ?? state?.currentProviderId ?? "正在检测"}</h1><p className="hero-detail">{state ? `${state.currentProviderId} · ${state.currentModel || "未设置模型"}` : "正在读取 ~/.codex/config.toml"} {state && <span className={`status-pill ${state.configConformant ? "" : "warning"}`}><span className={`dot ${state.configConformant ? "" : "warn"}`} />{state.configConformant ? "配置一致" : "配置需修复"}</span>}</p></div><div className="hero-actions"><button className="ghost-btn" onClick={() => void loadState()} disabled={busy}><RefreshCw size={15} />刷新状态</button>{state && !state.officialLoggedIn && selectedId === "official" && <button className="ghost-btn" onClick={loginOfficial} disabled={busy}><LogIn size={15} />登录 OpenAI</button>}<button className="primary-btn" onClick={switchChannel} disabled={!canSwitch}>{busy ? <RefreshCw size={15} className="spin" /> : <Wifi size={15} />}{isCurrent && selectedModel === state?.currentModel ? "重新验证并覆盖" : "切换并覆盖旧任务"}</button></div></section>

      {error && <div className="error-banner"><AlertTriangle size={17} /><div><strong>操作未完成</strong><span>{error}</span></div><button onClick={() => setError(null)}><X size={15} /></button></div>}

      <div className="grid">
        <section className="panel channels-panel"><div className="panel-head"><div><h2>目标渠道与模型</h2><p>所有状态来自本机真实配置和服务端能力</p></div><button className="add-btn" onClick={() => editChannel()}><Plus size={15} />添加渠道</button></div>
          <div className="channel-list">{allChannels.map((channel) => <div className={`channel-card ${channel.id === selectedId ? "selected" : ""}`} key={channel.id} onClick={() => setSelectedId(channel.id)}><div className={`channel-icon ${channel.id === "official" ? "official" : channel.id === "ailink" ? "ailink" : "custom"}`}>{channel.id === "official" ? <Sparkles size={18} /> : channel.id === "ailink" ? <Cloud size={18} /> : <Plus size={18} />}</div><div className="channel-info"><strong>{channel.name}{activeId === channel.id && <small>当前</small>}</strong><span>{channel.id === "official" ? (state?.officialLoggedIn ? "ChatGPT 官方账号已登录" : "需要登录 ChatGPT 官方账号") : `${channel.baseUrl} · ${channel.hasSecret ? "密钥已保存" : "缺少密钥"}`}</span></div><div className="channel-right">{channel.id !== "official" && <button className="mini-btn" title="编辑" onClick={(event) => { event.stopPropagation(); editChannel(channel); }}><Pencil size={13} /></button>}{!channel.isBuiltIn && channel.id !== "official" && <button className="mini-btn danger" title="删除" onClick={(event) => { event.stopPropagation(); setPendingDelete(channel); }}><Trash2 size={13} /></button>}{channel.id === selectedId ? <span className="selected-check"><Check size={14} /></span> : <ChevronRight size={16} className="muted" />}</div></div>)}</div>
          <div className="model-picker"><div><strong>目标模型</strong><span>{modelsLoading ? "正在读取服务端模型…" : modelError ?? modelNotice ?? `${models.length} 个可用模型`}</span></div><select value={selectedModel} onChange={(event) => setSelectedModel(event.target.value)} disabled={modelsLoading || !!modelError}>{models.map((model) => <option value={model.id} key={model.id}>{model.displayName || model.id}{model.isDefault ? "（默认）" : ""}</option>)}{!models.length && selectedModel && <option value={selectedModel}>{selectedModel}</option>}</select><button className="icon-btn" title="刷新模型" onClick={() => void loadModels(selectedId, selectedModel, selectedId === "official" || selected.validatesModelList)}><RefreshCw size={15} className={modelsLoading ? "spin" : ""} /></button></div>
        </section>

        <section className="side-stack">
          <div className="panel quota-panel"><div className="panel-head compact"><div><h2>真实额度</h2><p>{active?.name ?? "当前渠道"} · 每 10 秒更新</p></div><button className="icon-btn" aria-label="刷新额度" onClick={() => void refreshUsage()}><RefreshCw size={15} className={usageLoading ? "spin" : ""} /></button></div><UsageView usage={usage} error={usageError} /><label className="widget-mode"><span>额度胶囊</span><select value={state?.dockMode ?? "off"} onChange={(event) => void setWidgetMode(event.target.value as AppState["dockMode"])}><option value="free">自由悬浮</option><option value="edge">靠边隐藏</option><option value="off">关闭悬浮</option></select></label></div>
          <div className="panel checks-panel"><div className="panel-head compact"><div><h2>当前状态</h2><p>配置与凭据实时检查</p></div><ShieldCheck size={20} className={state?.configConformant ? "green" : "orange"} /></div><StatusRow ok={!!state?.configExists} title="配置文件" detail={state?.configExists ? "已读取" : "尚未创建"} /><StatusRow ok={!!state?.configConformant} title="Provider 配置" detail={state?.configConformant ? "一致" : "需要重新切换修复"} /><StatusRow ok={selectedId === "official" ? !!state?.officialLoggedIn : !!selected.hasSecret} title={selectedId === "official" ? "官方登录" : "渠道密钥"} detail={selectedId === "official" ? (state?.officialLoggedIn ? "ChatGPT" : "未登录") : (selected.hasSecret ? "系统凭据库" : "未保存")} /></div>
        </section>
      </div>

      {report && <section className="panel report-panel"><div className="panel-head"><div><h2>最近一次切换报告</h2><p>{report.providerId} · {report.model} · ${report.imageSkill}</p></div><span className="report-ok"><Check size={15} />已完成</span></div><div className="report-grid">{report.checks.map((check, index) => <div className={`report-row ${check.state}`} key={`${check.title}-${index}`}><span>{check.state === "passed" ? <Check size={14} /> : <AlertTriangle size={14} />}</span><div><strong>{check.title}</strong><small>{check.detail}</small></div></div>)}</div></section>}

      <section className="panel bottom-panel"><div className="bottom-item"><div className="bottom-icon"><KeyRound size={17} /></div><div><strong>系统安全存储</strong><span>API Key 不写入 config.toml 或前端存储</span></div></div><div className="bottom-item"><div className="bottom-icon"><Database size={17} /></div><div><strong>配置与任务双备份</strong><span>SQLite 事务只覆盖用户任务</span></div></div><div className="bottom-item"><div className="bottom-icon"><Copy size={17} /></div><div><strong>生图路由</strong><span>当前默认 ${state?.imageSkill ?? "读取中"}</span></div></div></section>
      <div className="statusbar"><span><span className={`dot ${error ? "warn" : ""}`} />{message}</span><span>免费开发模式 · 未配置正式代码签名</span></div>
    </main>

    {draft && <div className="modal-backdrop"><div className="modal"><div className="modal-head"><div><h2>{state?.channels.some((channel) => channel.id === draft.id) ? `编辑 ${draft.name}` : "添加自定义渠道"}</h2><p>Codex 自定义 Provider 固定使用 Responses API</p></div><button className="icon-btn" onClick={() => setDraft(null)}><X size={18} /></button></div><label>渠道名称<input value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label><label>API 地址<input value={draft.baseUrl} onChange={(event) => setDraft({ ...draft, baseUrl: event.target.value })} placeholder="https://example.com" /></label><label>API 密钥<input type="password" value={draft.secret} onChange={(event) => setDraft({ ...draft, secret: event.target.value })} placeholder={draft.hasSecret ? "留空则保留已保存密钥" : "请输入密钥"} autoComplete="new-password" /></label><div className="form-row"><label>默认模型<input value={draft.model} onChange={(event) => setDraft({ ...draft, model: event.target.value })} /></label><label>协议<input value="Responses" disabled /></label></div><div className="form-row"><label>模型列表路径<input value={draft.modelsPath} onChange={(event) => setDraft({ ...draft, modelsPath: event.target.value })} /></label><label>余额路径<input value={draft.usagePath} onChange={(event) => setDraft({ ...draft, usagePath: event.target.value })} placeholder="留空表示不查询余额" /></label></div><label className="toggle-row"><input type="checkbox" checked={draft.validatesModelList} onChange={(event) => setDraft({ ...draft, validatesModelList: event.target.checked })} />切换前校验服务端模型列表</label><div className="secret-note"><KeyRound size={15} /><span>{draft.hasSecret ? "已有密钥保存在系统凭据库。输入新值会覆盖，留空保持不变。" : "密钥将写入 macOS Keychain 或 Windows Credential Manager，不会返回给界面。"}</span></div><div className="modal-actions">{draft.hasSecret && <button className="danger-btn" onClick={() => setConfirmSecretDelete(true)}>删除密钥</button>}<button className="ghost-btn" onClick={() => setDraft(null)}>取消</button><button className="primary-btn" onClick={() => void saveChannel()} disabled={busy}>保存渠道</button></div></div></div>}

    {infoPanel && <div className="modal-backdrop"><div className="modal info-modal"><div className="modal-head"><div><h2>{infoPanel === "help" ? "Modelay 使用帮助" : "Modelay 设置"}</h2><p>{infoPanel === "help" ? "渠道切换、旧任务覆盖与安全边界" : "当前运行环境、本地数据与软件更新"}</p></div><button className="icon-btn" onClick={() => setInfoPanel(null)}><X size={18} /></button></div>{infoPanel === "help" ? <div className="info-list"><div><strong>切换渠道</strong><span>选择渠道和服务端实际可用模型后，Modelay 会先备份，再写配置、验证服务并覆盖用户任务索引。</span></div><div><strong>继续旧任务</strong><span>只覆盖用户任务的 Provider 和模型，不修改消息、rollout、子代理、自动审查或 Ollama 任务。</span></div><div><strong>生图路由</strong><span>官方渠道使用 $imagegen；AiLink 和自定义渠道使用 $imagegen2。</span></div><div><strong>软件更新</strong><span>启动后自动检查签名更新，也可以在设置中手动检查；安装前不会打断当前任务。</span></div><div><strong>出现错误</strong><span>切换事务会恢复配置、环境变量、偏好和生图路由；可从备份目录检查原始文件。</span></div></div> : <div className="info-list"><div><strong>当前 Provider</strong><span>{state?.currentProviderId ?? "读取中"} · {state?.currentModel || "未设置模型"}</span></div><div><strong>应用平台</strong><span>{state?.platform ?? "读取中"}</span></div><div><strong>备份目录</strong><span className="path-text">{state?.backupDirectory ?? "读取中"}</span></div><div><strong>安全存储</strong><span>{state?.platform.startsWith("windows") ? "Windows Credential Manager" : "macOS Keychain"}</span></div><div className="update-setting"><strong>软件更新 · {appVersion}</strong><span>{updateMessage}</span>{updatePhase === "downloading" || updatePhase === "installing" ? <div className="update-progress"><i style={{ width: `${updateProgress ?? 12}%` }} /></div> : null}<button className="ghost-btn info-action" onClick={() => void (updatePhase === "available" ? setUpdateOpen(true) : checkForUpdates(true))} disabled={updatePhase === "checking" || updatePhase === "downloading" || updatePhase === "installing"}>{updatePhase === "checking" ? <RefreshCw size={15} className="spin" /> : updatePhase === "available" ? <Download size={15} /> : <RefreshCw size={15} />}{updatePhase === "available" ? `安装 ${updateVersion}` : "检查更新"}</button></div><button className="ghost-btn info-action" onClick={() => void invoke("open_backup_folder")}><FolderOpen size={15} />打开备份目录</button></div>}</div></div>}

    {pendingDelete && <div className="modal-backdrop"><div className="modal confirm-modal"><div className="restart-icon danger-icon"><Trash2 size={21} /></div><h2>删除 {pendingDelete.name}？</h2><p>将删除渠道资料、系统凭据和残留环境变量。备份文件不会被删除。</p><div className="modal-actions"><button className="ghost-btn" onClick={() => setPendingDelete(null)}>取消</button><button className="danger-btn" onClick={() => void deleteChannel(pendingDelete)} disabled={busy}>确认删除</button></div></div></div>}

    {confirmSecretDelete && draft && <div className="modal-backdrop nested-modal"><div className="modal confirm-modal"><div className="restart-icon danger-icon"><KeyRound size={21} /></div><h2>删除 {draft.name} 的密钥？</h2><p>删除后该渠道将无法查询模型、余额或执行切换，直到重新保存密钥。</p><div className="modal-actions"><button className="ghost-btn" onClick={() => setConfirmSecretDelete(false)}>取消</button><button className="danger-btn" onClick={() => void deleteSecret()} disabled={busy}>确认删除密钥</button></div></div></div>}

    {restartOpen && <div className="modal-backdrop"><div className="modal restart-modal"><div className="restart-icon"><RefreshCw size={22} /></div><h2>真实配置已切换</h2><p>配置、诊断和用户任务覆盖已经完成。立即重启会中断 ChatGPT 中仍在运行的任务。</p><div className="modal-actions"><button className="ghost-btn" onClick={() => { setRestartOpen(false); setMessage("配置已生效，请稍后手动重启 ChatGPT"); }}>稍后手动重启</button><button className="primary-btn" onClick={() => void restartNow()}>立即重启</button></div></div></div>}
    {updateOpen && updateVersion && <div className="modal-backdrop"><div className="modal update-modal"><div className="restart-icon"><Download size={22} /></div><h2>Modelay {updateVersion} 可用</h2><p>当前版本 {appVersion}。更新包会先验证 Modelay 的数字签名，验证失败将拒绝安装。</p>{updateNotes && <div className="release-notes">{updateNotes}</div>}{updatePhase === "downloading" || updatePhase === "installing" ? <div className="modal-update-progress"><div><i style={{ width: `${updateProgress ?? 12}%` }} /></div><span>{updateMessage}{updateProgress !== null ? ` · ${updateProgress}%` : ""}</span></div> : <div className="update-warning"><AlertTriangle size={15} /><span>安装完成后 Modelay 会自动重启，请先结束正在执行的重要操作。</span></div>}<div className="modal-actions"><button className="ghost-btn" onClick={() => setUpdateOpen(false)} disabled={updatePhase === "downloading" || updatePhase === "installing"}>稍后提醒</button><button className="primary-btn" onClick={() => void installUpdate()} disabled={updatePhase === "downloading" || updatePhase === "installing"}>{updatePhase === "downloading" || updatePhase === "installing" ? <RefreshCw size={15} className="spin" /> : <Download size={15} />}立即更新并重启</button></div></div></div>}
  </div>;
}

function StatusRow({ ok, title, detail }: { ok: boolean; title: string; detail: string }) {
  return <div className={`check-row ${ok ? "" : "bad"}`}>{ok ? <Check size={15} /> : <AlertTriangle size={15} />}<span>{title}</span><em>{detail}</em></div>;
}

function UsageView({ usage, error }: { usage: UsageSnapshot | null; error: string | null }) {
  if (!usage && error) return <div className="usage-empty"><AlertTriangle size={16} /><span>{error}</span></div>;
  if (!usage) return <div className="usage-empty"><RefreshCw size={16} className="spin" /><span>正在读取额度…</span></div>;
  if (usage.kind === "official") return <div className="real-usage"><Quota label={quotaLabel(usage.fiveHour, "short")} window={usage.fiveHour} /><Quota label={quotaLabel(usage.weekly, "weekly")} window={usage.weekly} />{usage.creditsBalance && <div className="credit-line">可用 Credits：{usage.creditsBalance}</div>}{error && <small className="usage-warning">更新失败，保留上次数据：{error}</small>}</div>;
  return <div className="real-usage balance"><strong>{usage.balanceLabel ?? "可用余额"}</strong><b>{usage.remainingBalance?.toLocaleString(undefined, { maximumFractionDigits: 4 }) ?? "—"}</b><span>{usage.planName ?? "第三方渠道"}</span>{error && <small className="usage-warning">更新失败，保留上次数据：{error}</small>}</div>;
}

function Quota({ label, window: quota }: { label: string; window?: UsageWindow }) {
  const reset = quota?.resetsAt ? new Date(quota.resetsAt * 1000).toLocaleString() : "未知";
  return <div className="quota-line" title={`${label}重置时间：${reset}`}><span>{label}</span><div><i style={{ width: `${quota?.remainingPercent ?? 0}%` }} /></div><b>{quota ? `${Math.round(quota.remainingPercent)}%` : "—"}</b></div>;
}

export default App;
