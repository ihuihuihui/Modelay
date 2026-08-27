# Modelay 技术文档

## 架构

Modelay 采用 Tauri 2 桌面壳、React/TypeScript 界面和 Rust 核心。Rust 模块按职责拆分：`config` 管理 TOML，`secrets` 管理系统凭据，`codex` 调用 Codex CLI/app-server，`sessions` 管理 SQLite 任务索引，`usage` 解析额度，`platform` 处理 macOS/Windows 差异，`storage` 管理偏好与原子文件写入，`commands` 对前端提供稳定命令。

## 数据与安全

- Codex 配置：用户目录 `.codex/config.toml`。
- Codex 任务索引：`.codex/state_5.sqlite`。
- Modelay 偏好：系统应用数据目录的 `Modelay/preferences.json`。
- 备份：Modelay 应用数据目录的 `Backups`。
- 密钥：macOS Keychain 或 Windows Credential Manager，service 为 `app.modelay.desktop`。

偏好文件不包含密钥。第三方渠道必须使用 HTTPS，localhost 可使用 HTTP。自定义 Provider 固定使用 Responses API。

首次启动迁移兼容 CodexSwitch 3.x 的 `channels` 偏好结构，也兼容 2.x 的 `aiLink` 对象和独立 `ailink.json`。迁移读取旧 Keychain/环境变量，但不删除旧文件、旧密钥或旧备份。

## Provider 配置

官方模式移除顶层 `model_provider`。任务覆盖时优先从最近的用户官方任务识别实际 Provider，无法识别时使用 `openai_http`。

AiLink 使用 `custom`，自定义渠道使用 `custom_<安全化渠道 ID>`：

```toml
model_provider = "custom"
model = "gpt-5.6-sol"

[model_providers.custom]
name = "AiLink"
base_url = "https://ai.ailink1.com"
env_key = "AILINK_API_KEY"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
```

明文 `experimental_bearer_token` 会被移除。TOML 使用 `toml_edit` 修改，因此不会重建整个文件。

## 切换事务

1. 校验 URL、密钥和模型。
2. 在任何真实写入之前保存配置备份和 SQLite 在线一致性备份，并读取旧环境变量、生图路由和偏好。
3. 原子写入新配置，清除非目标第三方渠道的环境变量，仅注入目标渠道，并设置生图路由。
4. 运行 `codex doctor --json`，解析 `config.load` 与 `auth.credentials`；终端等非关键警告不会误阻断切换。
5. 验证官方登录/模型，或第三方模型和服务可达性。
6. 保存新的 Modelay 偏好。
7. 备份 SQLite，并在 `BEGIN IMMEDIATE` 事务中覆盖任务。
8. 返回结构化检查报告和重启提示。

数据库覆盖是最后一个可能失败的持久化步骤。前置步骤失败会恢复配置、全部渠道环境变量、生图路由和偏好；SQLite 自身失败由事务回滚。渠道资料和密钥的增删改也采用补偿事务：偏好、系统凭据与活跃渠道环境变量任一步失败，都会恢复之前状态。补偿动作本身如有失败，会合并进最终错误并要求从备份恢复，不会静默声称回滚完成。

所有会修改渠道、密钥或真实切换状态的后端操作共享进程级互斥锁，避免用户连续点击造成两个切换事务交错。生图路由回滚保存原文件的精确快照；切换前不存在该文件时，失败回滚会恢复为“不存在”，而不是写入推测默认值。

## 任务覆盖规则

只匹配 `openai*`、`custom` 和 `custom_*`。当前数据库要求 `thread_source='user'`，并排除空预览、`codex-auto-review` 和 subagent。旧数据库缺少 `thread_source` 时使用可见任务规则。Ollama 等其他 Provider、rollout 和消息历史不修改。

## 模型、额度与胶囊

- 官方模型：Codex app-server `model/list`。
- 第三方模型：Bearer 认证请求渠道 `modelsPath`。
- 官方额度：`account/rateLimits/read`，兼容单桶和多桶响应。存在 `rateLimitsByLimitId.codex` 时明确优先 Codex 桶，避免误用 `base_model_inference` 等同周期额度；界面按 `windowDurationMins` 动态显示 5 小时、15 分钟、1 小时、周额度等真实标签。
- 第三方余额：兼容 `remaining`、`balance` 和 `quota.remaining`。

Codex 子进程的 stdout/stderr 会在独立线程持续读取，避免输出填满管道后阻塞；命令超时会终止并回收子进程。Doctor 启动时会显式移除所有非目标渠道环境变量，只注入目标渠道密钥；输出在截断前同时按当前完整密钥、Bearer 认证头、常见 JSON 凭据字段和 `sk-` 形式脱敏。app-server RPC 无论成功、写入失败或超时都会关闭 stdin、终止进程并等待读取线程退出，避免周期性额度和模型请求累积孤儿进程。

第三方服务检查会阻断 401/403 密钥拒绝、429 限流和 5xx 服务故障；404/405 仍可视为主服务可达，适用于未实现模型列表路径且关闭了模型校验的 Responses 中转站。服务错误正文如果回显当前密钥，会在进入错误报告前按完整密钥脱敏。

额度胶囊是独立的 `usage` Tauri 窗口。它通过轻量 `get_widget_state` 读取 Provider、渠道和悬浮模式，不执行登录、Keychain 全量状态或 Codex CLI 检查；网络和文件系统命令统一放入 Tauri 异步阻塞线程池，避免卡住事件循环。`edge` 模式按显示器 scale factor 计算多屏工作区，拖到 48pt 内吸附，隐藏后保留 10pt，鼠标离开 650ms 后收起。自由悬浮和靠边隐藏都会持久化位置，并在显示器移除或分辨率变化后把窗口钳制回当前工作区。数据每 10 秒刷新，失败保留上次成功值并显示可诊断错误。

主窗口的关闭按钮只隐藏窗口，应用继续提供额度胶囊。托盘菜单和托盘左键可恢复主窗口；macOS Dock 再次打开事件及第二次启动同一应用也会恢复现有窗口。应用使用 single-instance 插件阻止重复后台实例，额度胶囊自身仍不承担打开主界面的行为。

边缘计算被拆为纯函数测试，覆盖自由位置、左/右/上/下四边、10pt 留边、Retina 物理像素缩放和越界钳制。应用在“靠边隐藏”模式启动时会重新计算当前位置并恢复隐藏状态。

## 平台适配

macOS 使用 Keychain、`launchctl setenv`、ChatGPT 应用探测和 `open -a ChatGPT`。旧 CodexSwitch 密钥使用禁止认证弹窗的非交互 Keychain 查询迁移，避免启动阻塞。额度胶囊添加 `NSWindowStyleMaskNonactivatingPanel` 和全空间辅助窗口行为。

Windows 使用 Credential Manager、`HKCU\\Environment` 和 `WM_SETTINGCHANGE` 广播环境更新，探测常见安装目录、运行中的 ChatGPT 进程和 Microsoft Store/MSIX 包；重启时优先复用可执行文件，必要时通过 Windows App ID 启动。额度胶囊添加 `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`，并通过 `SetWindowPos(..., SWP_FRAMECHANGED)` 立即应用扩展样式。

## 测试、构建与更新

Rust 单元测试覆盖 TOML 保留、原子覆盖、精确文件快照、Provider/URL、旧版迁移、Doctor、官方/第三方额度、Codex 多额度桶优先级、Provider 动态识别、数据库锁回滚，以及当前/旧版 SQLite 表结构。TypeScript 测试覆盖悬浮窗边缘几何、动态额度周期标签、模型选择和更新错误/进度状态。GitHub Actions 在 Linux 运行这些测试，并在 macOS、Windows 生成应用包；Windows Rust/Win32 源码也使用 `cargo-xwin` 与 Windows CRT/SDK 完成交叉编译检查。

当前自动验证基线为 Rust 23 项单元测试和 TypeScript 13 项悬浮窗/额度标签/模型选择/更新状态测试全部通过；Rust Clippy 全目标零警告、TypeScript 类型检查、Vite 生产构建、npm 生产依赖高危漏洞检查及 `x86_64-pc-windows-msvc` 交叉检查通过。Windows 交叉检查验证 Rust/Win32 源码，Windows runner 已成功生成 `v4.0.0-alpha.3` NSIS 安装器；运行行为仍需 Windows 实机验收。

`tauri.windows.conf.json` 将 Windows 默认 bundle 设为 NSIS 与 MSI。`scripts/package-windows.ps1` 负责复制安装器并生成 SHA-256 文件；macOS 脚本会自动识别 arm64/x64、执行 ad-hoc 签名、严格签名验证、DMG 验证并生成 SHA-256 文件。两平台安装包统一写入顶层 `artifacts/installers`，不再放进会被 Vite 清空的 `dist` 目录。常规 CI 构建两平台安装包；版本标签发布工作流会校验标签与 `package.json` 版本完全一致，再由官方 Tauri Action 创建 GitHub Release、签名更新包和 `latest.json`。

Windows bundle 目标由 `scripts/windows-bundles.mjs` 根据版本选择：正式版本和纯数字预发布版本生成 NSIS 与 MSI；包含 `alpha`、`beta` 等文字预发布标识时仅生成 NSIS，因为 WiX/MSI 的 ProductVersion 不接受非数字预发布字段。NSIS 是 Windows 自动更新首选产物，不影响应用内升级；正式版本仍会恢复 MSI。

Updater 已在 Rust 运行时注册，并通过最小权限开放检查、下载和安装命令。主窗口启动 4 秒后自动检查一次，设置页也可手动检查；发现新版本后显示版本号、发行说明和下载进度，安装结束后调用进程插件重启。下载内容必须通过嵌入公钥验证，签名错误会明确拒绝安装。常规本地构建继续关闭 `createUpdaterArtifacts`；只有发布配置 `tauri.release.generated.json` 临时开启它并注入 HTTPS 更新端点，避免开发包意外连接不存在的发布源。

更新签名私钥为加密文件，保存在 `/Users/Admin/Library/Application Support/Modelay Development/updater/modelay-updater.key`，权限为 `0600`；密码保存在 macOS Keychain 的 `app.modelay.desktop updater signing` 项。仓库只包含公钥。GitHub Actions 使用 `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 两项 Secret，更新端点在构建时根据 `${{ github.repository }}` 生成。该更新签名独立于 Apple Developer ID 和 Windows Authenticode 代码签名。

`scripts/check-version.mjs` 会校验 `package.json`、`tauri.conf.json` 和 `Cargo.toml` 的版本完全一致；本地 `npm run verify`、常规 CI、打包任务及标签发布门禁都会执行该检查，防止生成名称与内部版本不一致的安装器。

当前 macOS 构建已通过 `.app` ad-hoc 深度签名严格校验和 DMG 完整性校验。只读 UI 冒烟已验证真实 AiLink 状态、服务端动态模型、实时钱包余额、帮助/设置弹窗、可编辑且不回显旧值的密钥输入框、主窗口关闭后的独立胶囊、点击胶囊不唤起主界面、“靠边隐藏”自动收起、关闭主窗口转入后台、再次启动唤回以及进程数保持单实例；验收完成后悬浮偏好已恢复为“自由悬浮”。该冒烟过程未调用渠道切换、任务覆盖或 ChatGPT 重启。

当前发布测试包（`v4.0.0-alpha.3`）校验值：

| 文件 | SHA-256 |
| --- | --- |
| `Modelay_4.0.0-alpha.3_aarch64.dmg` | `3863c8b3e0b912ca27e11222b5917b35708556afc080781437727b3507218581` |
| `Modelay_4.0.0-alpha.3_x64-setup.exe` | `5936a329fcbb626666a98743b9fe1dd5534fcc8e4a8c2b9ced2b65ab3fb4d057` |

免费开发阶段已经启用 Tauri 更新包的独立签名验证，但仍不包含正式 Apple/Windows 代码签名。macOS 测试包会执行 ad-hoc 深度签名并严格校验，DMG 通过 `hdiutil verify`。公开仓库 `ihuihuihui/Modelay` 已配置发布所需的 GitHub Actions Secrets，`v4.0.0-alpha.3` 的 macOS/Windows 签名更新包与 `latest.json` 已发布并通过公开下载验证。私钥不得提交到仓库，也不能在已有安装用户后随意更换，否则旧版本将无法验证后续更新。
