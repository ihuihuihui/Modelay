# Modelay 技术文档

## 架构

Modelay 采用 Tauri 2 桌面壳、React/TypeScript 界面和 Rust 核心。Rust 模块按职责拆分：`config` 管理 TOML，`secrets` 管理系统凭据，`codex` 调用 Codex CLI/app-server，`sessions` 管理 SQLite 任务索引，`handoff` 检查会话健康并生成精简交接，`usage` 解析额度，`platform` 处理 macOS/Windows 差异，`storage` 管理偏好与原子文件写入，`commands` 对前端提供稳定命令。

## 数据与安全

- Codex 配置：用户目录 `.codex/config.toml`。
- Codex 任务索引：`.codex/state_5.sqlite`。
- Modelay 偏好：系统应用数据目录的 `Modelay/preferences.json`。
- 备份：Modelay 应用数据目录的 `Backups`。
- 密钥：macOS Keychain 或 Windows Credential Manager，service 为 `app.modelay.desktop`。

偏好文件不包含密钥。第三方渠道必须使用 HTTPS，localhost 可使用 HTTP。自定义 Provider 固定使用 Responses API。

公开版首次启动创建空的第三方渠道列表，因此主界面只显示官方 Codex。首次启动不会扫描或导入 CodexSwitch 偏好、旧 `ailink.json`、旧 Keychain 或第三方环境变量；客户必须通过“添加渠道”自行填写 API 地址、模型、接口路径和密钥。升级安装继续读取 Modelay 自己的偏好与系统凭据，因此已有用户的渠道不会被删除或重置。

## Provider 配置

官方模式移除顶层 `model_provider`。任务覆盖时优先从最近的用户官方任务识别实际 Provider，无法识别时使用 `openai_http`。

自定义渠道使用 `custom_<安全化渠道 ID>`：

```toml
model_provider = "custom_example"
model = "gpt-5.6-sol"
model_reasoning_effort = "medium"

[model_providers.custom_example]
name = "Example Relay"
base_url = "https://api.example.com"
env_key = "CODEX_EXAMPLE_API_KEY"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
```

为兼容已经安装的旧版 Modelay，既有 `ailink` 渠道 ID 仍映射为 `custom` / `AILINK_API_KEY`，但公开版不会主动创建或导入该渠道。

明文 `experimental_bearer_token` 会被移除。TOML 使用 `toml_edit` 修改，因此不会重建整个文件。

## 切换事务

1. 校验 URL、密钥、模型和该模型支持的推理强度。
2. 在任何真实写入之前保存配置备份和 SQLite 在线一致性备份，并读取旧环境变量、生图路由和偏好。
3. 原子写入新配置，设置目标渠道环境变量并更新生图路由；清除所有非目标第三方环境变量。每个 Codex 子进程仍会先清空全部 Provider 密钥再只注入目标渠道，避免诊断和运行串用凭据。
4. 运行 `codex doctor --json`，解析 `config.load` 与 `auth.credentials`；终端等非关键警告不会误阻断切换。
5. 验证官方登录/模型，或第三方模型和服务可达性。
6. 保存新的 Modelay 偏好。
7. 备份 SQLite，并在 `BEGIN IMMEDIATE` 事务中覆盖任务的 Provider、模型和推理强度。
8. 返回结构化检查报告和重启提示。

数据库覆盖是最后一个可能失败的持久化步骤。前置步骤失败会恢复配置、全部渠道环境变量、生图路由和偏好；SQLite 自身失败由事务回滚。渠道资料和密钥的增删改也采用补偿事务：偏好、系统凭据与活跃渠道环境变量任一步失败，都会恢复之前状态。补偿动作本身如有失败，会合并进最终错误并要求从备份恢复，不会静默声称回滚完成。

所有会修改渠道、密钥或真实切换状态的后端操作共享进程级互斥锁，避免用户连续点击造成两个切换事务交错。生图路由回滚保存原文件的精确快照；切换前不存在该文件时，失败回滚会恢复为“不存在”，而不是写入推测默认值。

## 任务覆盖规则

只匹配 `openai*`、`custom` 和 `custom_*`。当前数据库要求 `thread_source='user'`，并排除空预览、`codex-auto-review` 和 subagent。旧数据库缺少 `thread_source` 时使用可见任务规则；旧表缺少 `reasoning_effort` 时仍完成 Provider 与模型覆盖。Ollama 等其他 Provider、rollout 和消息历史不修改。

跨渠道切换默认采用智能续接：旧任务保持原 Provider 绑定，切换目标渠道后从指定任务提取精简交接内容并创建新任务，避免目标 Provider 重新处理完整长上下文。用户也可仅切换渠道，或显式选择最近 5 个、全部、指定用户任务范围，把旧任务的 Provider、模型和推理强度改为目标配置；直接迁移前会生成 SQLite 备份。

## 模型、额度与胶囊

- 官方模型：Codex app-server `model/list`。
- 第三方模型：Bearer 认证请求渠道 `modelsPath`。
- 推理强度：读取官方模型的 `supportedReasoningEfforts`，界面提供即时、快速、平衡、深度、极深和最大档位；第三方未声明能力时保留手动选择。切换事务会把结果写入顶层 `model_reasoning_effort` 和用户任务索引。
- 官方额度：`account/rateLimits/read`，兼容单桶和多桶响应。存在 `rateLimitsByLimitId.codex` 时明确优先 Codex 桶，避免误用 `base_model_inference` 等同周期额度；界面按 `windowDurationMins` 动态显示 5 小时、15 分钟、1 小时、周额度等真实标签。
- 第三方余额：兼容 `remaining`、`balance` 和 `quota.remaining`。

Codex 子进程的 stdout/stderr 会在独立线程持续读取，避免输出填满管道后阻塞；命令超时会终止并回收子进程。Doctor 启动时会显式移除所有非目标渠道环境变量，只注入目标渠道密钥；完整输出先按当前密钥、Bearer 认证头、常见 JSON 凭据字段和 `sk-` 形式脱敏，再解析完整 JSON，只有最终展示给用户的错误摘要才受字符上限限制。这样当前包含大量 feature flags、Git 和网络细节的 Doctor 报告不会在解析前被截断。

官方模型与额度 RPC 复用一个进程级持久 app-server，JSON-RPC 请求使用递增 ID 串行匹配；连接异常时回收旧进程并只重试一次。渠道切换前会关闭旧连接，确保新配置不会复用旧 Provider 状态；Modelay 退出时同步终止子进程并等待读取线程结束。主窗口与额度胶囊另共享 8 秒成功结果缓存，因此两个 10 秒刷新定时器不会重复请求同一份额度。缓存失败不覆盖上次界面数据，切换渠道时立即清空。

第三方服务检查会阻断 401/403 密钥拒绝、429 限流和 5xx 服务故障；404/405 仍可视为主服务可达，适用于未实现模型列表路径且关闭了模型校验的 Responses 中转站。服务错误正文如果回显当前密钥，会在进入错误报告前按完整密钥脱敏。

额度胶囊是独立的 `usage` Tauri 窗口。它通过轻量 `get_widget_state` 读取 Provider、渠道和悬浮模式，不执行登录、Keychain 全量状态或 Codex CLI 检查；网络和文件系统命令统一放入 Tauri 异步阻塞线程池，避免卡住事件循环。`edge` 模式按显示器 scale factor 计算多屏工作区，拖到 48pt 内吸附，隐藏后保留 10pt，鼠标离开 650ms 后收起。自由悬浮和靠边隐藏都会持久化位置，并在显示器移除或分辨率变化后把窗口钳制回当前工作区。数据每 10 秒刷新，失败保留上次成功值并显示可诊断错误。

主窗口的关闭按钮只隐藏窗口，应用继续提供额度胶囊。托盘菜单和托盘左键可恢复主窗口；macOS Dock 再次打开事件及第二次启动同一应用也会恢复现有窗口。应用使用 single-instance 插件阻止重复后台实例，额度胶囊自身仍不承担打开主界面的行为。

## 会话健康与自动续接

用户选择会话后，Modelay 从 Codex 任务索引读取累计 tokens、工作目录、模型、更新时间和 rollout 路径，并只解析该任务最后活动日的用户与助手消息。rollout 中最近一次 `token_count` 事件的输入达到 4 万 tokens 时进入警告级别，达到 8 万时进入严重级别并建议智能续接；累计 tokens、单日 rollout 体积和消息数量作为辅助风险指标。

创建续接任务时，Modelay 提取最后活动日的用户需求、助手进度以及消息中引用的绝对路径，生成长度受限的结构化交接 Prompt，再调用 Codex app-server 的 `thread/start` 和 `turn/start`。模型与 Provider 只在线程创建时设置，首轮沿用线程配置，避免第三方渠道重复解析模型覆盖参数。Codex 接受并持久化用户交接消息后，Modelay 尽力调用 `turn/interrupt` 停止空转并关闭自己的 app-server；轮次若已经提前停止，interrupt 错误不会把已经创建成功的续接任务误报为失败。`turn/start` 真正失败时会归档刚创建的空任务，并在续接区域显示脱敏后的具体原因与恢复建议。新任务不复制旧任务的完整消息历史、rollout 或数据库记录；旧任务不会被修改、删除或覆盖。任务索引兼容存在或缺少 `thread_source`、`updated_at_ms` 的数据库结构。

边缘计算被拆为纯函数测试，覆盖自由位置、左/右/上/下四边、10pt 留边、Retina 物理像素缩放和越界钳制。应用在“靠边隐藏”模式启动时会重新计算当前位置并恢复隐藏状态。

## 平台适配

macOS 使用 Keychain、`launchctl setenv`、ChatGPT 应用探测，并直接启动 ChatGPT 可执行文件和传递目标渠道环境变量。`alpha.5` 将渠道密钥写入新的 `app.modelay.desktop.v2` service，不再隐式读取可能绑定旧 ad-hoc 身份的 `app.modelay.desktop` 项。状态读取优先使用渠道的用户会话环境和进程内缓存，再通过 Security Framework 非交互读取新 service；只有用户明确保存或更新密钥时才写入 Keychain，读取状态和切换不会在后台迁移密钥。切换渠道时清除所有非目标环境变量，Codex 子进程仍严格隔离非目标密钥。运行时不启动 `/usr/bin/security`。

公开 macOS 包另外使用固定的自签名开发代码签名证书。不同版本的 designated requirement 均为 `identifier "app.modelay.desktop"` 加同一证书根指纹，不再使用每次构建都会变化的 ad-hoc `cdhash`。证书私钥保存在 `/Users/Admin/Library/Application Support/Modelay Development/code-signing`，密码位于 macOS Keychain；GitHub Actions 只通过加密 Secrets 导入 P12。该签名用于稳定 Keychain 代码身份，仍不属于 Apple Developer ID，也不能替代 Gatekeeper 公证。

Windows 使用 Credential Manager、`HKCU\\Environment` 和 `WM_SETTINGCHANGE` 广播环境更新，探测常见安装目录、运行中的 ChatGPT 进程和 Microsoft Store/MSIX 包；Codex CLI 发现优先使用 `CODEX_CLI_PATH`、`%LOCALAPPDATA%\\OpenAI\\Codex\\bin` 下的用户态迁移版本、独立安装器版本目录和 npm `codex.cmd`，逐个试运行后才回退到可能受 WindowsApps ACL 保护的包内文件。重启时优先复用可执行文件，必要时通过 Windows App ID 启动。额度胶囊添加 `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`，并通过 `SetWindowPos(..., SWP_FRAMECHANGED)` 立即应用扩展样式。

## 测试、构建与更新

Rust 单元测试覆盖 TOML 保留、原子覆盖、精确文件快照、Provider/URL、全新安装仅官方渠道、已有 AiLink 渠道升级保留、Doctor、官方/第三方额度、Codex 多额度桶优先级、Provider 动态识别、数据库锁回滚，以及当前/旧版 SQLite 表结构。TypeScript 测试覆盖悬浮窗边缘几何、动态额度周期标签、模型选择和更新错误/进度状态。GitHub Actions 在 Linux 运行这些测试，并在 macOS、Windows 生成应用包；Windows Rust/Win32 源码也使用 `cargo-xwin` 与 Windows CRT/SDK 完成交叉编译检查。

当前自动验证基线包含 Windows 用户态 Codex 路径发现回归测试，以及既有 Rust 与 TypeScript 的悬浮窗、额度标签、模型选择、跨渠道范围、版本说明、更新状态、Doctor、Keychain、额度缓存、推理强度和会话交接测试。Rust Clippy 全目标零警告、TypeScript 类型检查与 Vite 生产构建通过。Windows runner 负责完整 Windows 编译和 NSIS 安装器生成；运行行为仍需 Windows 实机验收。

`tauri.windows.conf.json` 将 Windows 默认 bundle 设为 NSIS 与 MSI。`scripts/package-windows.ps1` 负责复制安装器并生成 SHA-256 文件；macOS 脚本会自动识别 arm64/x64，优先使用 CI 注入的固定开发签名身份（本地未注入时回退 ad-hoc），然后执行严格签名验证、DMG 验证并生成 SHA-256 文件。两平台安装包统一写入顶层 `artifacts/installers`，不再放进会被 Vite 清空的 `dist` 目录。常规 CI 构建两平台安装包；版本标签发布工作流会校验标签与 `package.json` 版本完全一致，再由官方 Tauri Action 创建 GitHub Release、签名更新包和 `latest.json`。

Windows bundle 目标由 `scripts/windows-bundles.mjs` 根据版本选择：正式版本和纯数字预发布版本生成 NSIS 与 MSI；包含 `alpha`、`beta` 等文字预发布标识时仅生成 NSIS，因为 WiX/MSI 的 ProductVersion 不接受非数字预发布字段。NSIS 是 Windows 自动更新首选产物，不影响应用内升级；正式版本仍会恢复 MSI。

Updater 已在 Rust 运行时注册，并通过最小权限开放检查、下载和安装命令。主窗口启动 4 秒后自动检查一次，设置页也可手动检查；发现新版本后显示版本号、发行说明和下载进度，安装结束后调用进程插件重启。下载内容必须通过嵌入公钥验证，签名错误会明确拒绝安装。常规本地构建继续关闭 `createUpdaterArtifacts`；只有发布配置 `tauri.release.generated.json` 临时开启它并注入 HTTPS 更新端点，避免开发包意外连接不存在的发布源。

所有公开 Release 与应用内更新说明使用中文撰写，在不牺牲准确性的前提下保持轻松、易读；发布工作流中的 `releaseBody` 不再使用英文模板。

更新签名私钥为加密文件，保存在 `/Users/Admin/Library/Application Support/Modelay Development/updater/modelay-updater.key`，权限为 `0600`；密码保存在 macOS Keychain 的 `app.modelay.desktop updater signing` 项。仓库只包含公钥。GitHub Actions 使用 `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 两项 Secret，更新端点在构建时根据 `${{ github.repository }}` 生成。该更新签名独立于 Apple Developer ID 和 Windows Authenticode 代码签名。

`scripts/check-version.mjs` 会校验 `package.json`、`tauri.conf.json` 和 `Cargo.toml` 的版本完全一致；本地 `npm run verify`、常规 CI、打包任务及标签发布门禁都会执行该检查，防止生成名称与内部版本不一致的安装器。

当前 macOS 发布构建已通过固定开发身份的 `.app` 深度签名严格校验、designated requirement 检查和 DMG 完整性校验。自动测试验证全新偏好只包含官方渠道，并验证已有 AiLink 渠道升级后仍作为用户渠道保留。此前在既有用户配置上完成的只读 UI 冒烟覆盖服务端动态模型、实时钱包余额、帮助/设置弹窗、可编辑且不回显旧值的密钥输入框、主窗口关闭后的独立胶囊、点击胶囊不唤起主界面、“靠边隐藏”自动收起、关闭主窗口转入后台、再次启动唤回以及进程数保持单实例；该过程未调用渠道切换、任务覆盖或 ChatGPT 重启。

免费开发阶段已经启用 Tauri 更新包的独立签名验证，但仍不包含正式 Apple Developer ID/Windows Authenticode 签名。macOS 测试包使用固定自签名开发身份执行深度签名并严格校验，DMG 通过 `hdiutil verify`。公开仓库 `ihuihuihui/Modelay` 已配置发布所需的 GitHub Actions Secrets；每次标签发布会同时生成 macOS/Windows 签名更新包和 `latest.json`。私钥不得提交到仓库，也不能在已有安装用户后随意更换，否则旧版本将无法验证后续更新。
