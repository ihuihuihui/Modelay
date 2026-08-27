# Modelay

Modelay 是面向 Codex/ChatGPT 桌面端的跨平台渠道与额度管理器，使用 Tauri 2、React、TypeScript 和 Rust 构建。

## 已实现

- OpenAI 官方与用户自行配置的 Responses 兼容第三方渠道切换。
- 原子修改 `~/.codex/config.toml`，保留 MCP、插件、注释和未知字段。
- macOS Keychain / Windows Credential Manager 安全保存 API Key，前端只读取 `hasSecret`。
- 官方/第三方动态模型、推理强度、真实额度、任务索引备份与用户任务 Provider/模型/推理强度覆盖。多额度桶会明确优先 Codex 桶，短周期标签按服务端 `windowDurationMins` 显示，不会把 15 分钟或 1 小时误写成 5 小时。
- 完整切换事务和失败回滚；官方 `$imagegen`，第三方 `$imagegen2`。
- 切换前同时生成 Codex 配置和 SQLite 一致性备份；任何自动回滚失败都会明确报告，不再静默忽略。
- 独立额度胶囊：自由悬浮、靠边隐藏、关闭悬浮、10 秒刷新和多屏位置持久化。
- 胶囊使用轻量状态接口，不会每 2 秒重复启动 Codex 登录检查；macOS/Windows 使用原生无激活窗口样式。
- 主窗口和胶囊共享 8 秒额度缓存；官方模型和额度查询复用同一个持久 Codex app-server，避免每 10 秒反复创建后台进程。切换渠道和退出 Modelay 时会安全回收连接。
- 推理强度支持即时、快速、平衡、深度、极深和最大档位，并按服务端模型能力过滤；默认使用平衡档，高强度模式会提示额外延迟与断线重试风险。
- 主窗口关闭后转入后台；系统托盘/再次启动可恢复主窗口，单实例保护避免重复运行。点击额度胶囊仍不会打开主界面。
- 活跃渠道修改或删除密钥时同步更新启动环境变量，失败会恢复原密钥、环境与偏好。
- 全新安装只显示官方 Codex，不内置第三方地址、模型或密钥，也不自动导入 CodexSwitch、环境变量或旧 Keychain；升级会保留用户已保存的 Modelay 渠道。
- 自定义渠道会校验 HTTPS、本机 HTTP、渠道 ID 和 Provider 冲突，避免生成不可区分的配置。
- Codex Doctor 在完整脱敏 JSON 上完成解析，超长诊断不会再因界面显示上限被截成无效 JSON。
- macOS 发布包使用固定的免费开发签名身份；新保存的渠道密钥改用独立 Keychain service，并保留进程缓存和用户会话环境，避免切换或自动额度刷新反复弹出钥匙串授权。升级后可能需要明确保存一次既有密钥，但不会在后台读取或迁移旧凭据。
- 切换后提供“立即重启 / 稍后手动重启”。
- 启动后自动检查签名更新，设置页支持手动检查、更新说明、下载进度、一键安装和自动重启；签名无效时拒绝安装。

旧版 CodexSwitch 3.x 保存在 `legacy/macos-3`，不会被删除。

## 本地开发

```bash
npm ci
npm run check:version
npm run build
npm test
npm run tauri dev
```

macOS 免费测试包：

```bash
npm run package:macos
```

输出位于 `artifacts/installers/Modelay-macOS-arm64.zip` 和 `Modelay-macOS-arm64.dmg`。脚本会在打包后执行严格签名校验；发布 CI 使用固定开发身份，本地未注入身份时回退 ad-hoc，适合本机和内部测试。安装包与 Vite 的 `dist` 前端目录完全分离，因此后续执行 `npm run build` 或 `npm run verify` 不会再删除安装包。

当前发布测试包（`v4.0.0-alpha.6`）：

| 文件 | 大小 | SHA-256 |
| --- | ---: | --- |
| `Modelay_4.0.0-alpha.6_aarch64.dmg` | 10.7 MB | `2a3d2238471539eb8a46a8edc34921910ae4cca983990f059339ce57ac50e9e2` |
| `Modelay_4.0.0-alpha.6_x64-setup.exe` | 5.0 MB | `4e65a844fa7f319fb3770931dea79a49538650827965364284010ddeb56932a7` |

本机 `artifacts/installers/Modelay_4.0.0-alpha.6_SHA256.txt` 可用于校验两个安装包的下载完整性。安装产物保存在顶层 `artifacts`，不会被 Vite 生产构建清除。

Windows 本机打包：

```powershell
npm ci
npm run package:windows
```

Windows 默认读取 `src-tauri/tauri.windows.conf.json` 并生成 NSIS `.exe` 与 MSI；产物及 `Modelay-Windows-SHA256.txt` 位于 `artifacts/installers`。也可以由 `.github/workflows/build.yml` 在 Windows runner 构建；未签名包可能触发 SmartScreen 提示。
Rust/Win32 代码已使用 `cargo-xwin` 和 Windows CRT/SDK 完成交叉编译检查，最终安装器链接与运行行为仍需 Windows CI 和实机验收。

## 免费发布准备

- `.github/workflows/build.yml` 会测试后同时生成 macOS ZIP/DMG 与 Windows NSIS/MSI，并附 SHA-256 文件。
- `.github/workflows/release.yml` 在推送与 `package.json` 版本一致的 `v*` 标签时创建 GitHub Release，例如 `v4.0.0-alpha.6`，并发布签名更新产物与 `latest.json`。
- 更新公钥已写入应用；加密私钥只保存在本机 `/Users/Admin/Library/Application Support/Modelay Development/updater/modelay-updater.key`，密码保存在 macOS Keychain，不进入仓库。
- 发布仓库需要公开读取 Release 资产，并在 GitHub Actions Secrets 中配置 `TAURI_SIGNING_PRIVATE_KEY` 与 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
- macOS 固定开发签名证书通过 `MODELAY_MACOS_CERTIFICATE` 与 `MODELAY_MACOS_CERTIFICATE_PASSWORD` 两项 Secret 注入；它只稳定免费包的代码身份，不等同于 Apple Developer ID 或公证。
- 当前公开发布仓库为 `ihuihuihui/Modelay`，`v4.0.0-alpha.6` 已完成 macOS 与 Windows 构建并上线公开更新清单。
- 旧的 `4.0.0-alpha.1` 安装包没有更新器，因此需要手动安装一次当前 `4.0.0-alpha.6`；`alpha.3` 及之后的用户可以直接在应用内升级。
- Windows 的非数字预发布版本生成 NSIS `.exe`；WiX/MSI 要求数字兼容版本，因此 MSI 从正式纯数字版本恢复生成。

## 当前验证结果

- Rust 单元测试：29 项通过。
- TypeScript 悬浮窗、额度标签、模型选择和更新状态测试：13 项通过。
- Rust Clippy 全目标零警告：通过。
- TypeScript 检查与 Vite 生产构建：通过。
- npm 生产依赖高危漏洞检查：0 项。
- Windows `x86_64-pc-windows-msvc` Rust/Win32 交叉检查：通过。
- macOS 发布包固定开发身份深度签名与 DMG 完整性校验：通过。
- macOS DMG `hdiutil verify`：通过。
- macOS 只读 UI 冒烟：真实渠道、动态模型、实时余额、帮助、设置、密钥编辑框、独立胶囊、点击不唤起主界面、靠边自动收起、关闭转后台、再次启动唤回和单实例均通过。

## 尚需人工验收

- 首次 OpenAI → 自定义渠道 → OpenAI 真实往返会修改 Codex 配置、任务数据库并重启 ChatGPT，需要用户在执行前确认。
- Windows 运行时行为需要 Windows 实机验收。
- Apple/Windows 正式代码签名仍需要相应证书，但不阻塞 Tauri 更新包的独立签名验证。
