# Codex Switch 技术文档

版本：3.0.1

平台：macOS 14+，当前构建目标为 Apple Silicon arm64

## 1. 项目概述

Codex Switch 是一个原生 macOS SwiftUI 工具，用于在 OpenAI 官方 ChatGPT 登录、AiLink 和任意 OpenAI 兼容中转站之间切换。它管理 Codex 的 `config.toml`、Keychain 密钥、用户环境变量、模型选择和切换后诊断，目标是减少手工改配置造成的 Provider 缺失、模型不兼容和反复重连。

| 渠道 | 顶层激活 Provider | 默认模型 | 生图路由 |
| --- | --- | --- | --- |
| OpenAI 官方 | 不设置 `model_provider = "custom"`，使用内置 `openai` | `gpt-5.6-sol` | `$imagegen` |
| AiLink | `model_provider = "custom"` | `gpt-5.5` | `$imagegen2` |
| 自定义渠道 | `model_provider = "custom_<渠道ID>"` | 用户填写 | `$imagegen2` |

## 2. 目标与边界

### 2.1 目标

- 官方或任意第三方渠道一键切换。
- 支持添加、编辑、删除多个自定义渠道，每个渠道独立保存地址、密钥、模型及兼容参数。
- 保留 MCP、插件、项目权限和其他非本工具配置。
- 切换前自动备份。
- API Key 存入 macOS Keychain，不把新密钥写入 `config.toml`。
- 使用 Codex Doctor、模型列表和 HTTP 检查验证结果。
- 失败自动恢复配置、环境变量和生图路由状态。
- 切换时批量覆盖全部本地 OpenAI/第三方任务的 Provider 和模型。
- 切换成功后弹出重启确认，由用户选择立即重启或稍后手动重启，使已经加载的旧任务释放旧设置。
- 常驻悬浮窗按当前渠道显示官方 5 小时/周额度或第三方余额接口返回值。
- 支持手动刷新，并每 10 秒近实时刷新额度。
- 微型胶囊悬浮窗支持自由悬浮、拖到边缘后自动收起的靠边隐藏，以及关闭悬浮。
- 悬浮窗使用非激活 `NSPanel`，交互时不唤起主窗口。

### 2.2 边界

Codex 会在会话创建时记录 Provider 和模型，仅修改全局配置不会改变旧任务。3.0.1 在切换事务中同步更新 `~/.codex/state_5.sqlite` 的任务索引，把原 Provider 为 `openai`、`custom` 或 `custom_*` 的全部任务覆盖为当前渠道和模型。应用不修改 rollout 历史、不创建 Fork，也不触发模型请求。其他 Provider（例如 Ollama）不受影响。

`state_5.sqlite` 属于 Codex 本地状态格式，不是稳定的公共配置接口。应用每次修改前使用 SQLite 在线备份；若未来 Codex 改变表结构，切换会明确失败并保留原配置与备份，不会静默修改未知结构。

应用记录 `$imagegen`/`$imagegen2` 作为默认生图路由提示，不修改 Codex 技能文件，也不会改变已经打开任务的上下文。

## 3. 技术路线

| 层次 | 技术 |
| --- | --- |
| UI | SwiftUI 原生 macOS `WindowGroup`、`Form`、`Sheet`、`ConfirmationDialog` |
| 系统集成 | AppKit `NSWorkspace`、浮动 `NSPanel` |
| 密钥 | Security.framework Keychain Generic Password |
| 配置 | Foundation 实现的 Codex 专用 TOML 定点编辑器 |
| 进程 | Foundation `Process` 调用 `codex`、`sqlite3` 和 `/bin/launchctl` |
| 网络 | Foundation `URLSession` 调用各渠道配置的模型列表、余额路径与 HTTPS 端点 |
| 官方额度 | Codex `app-server --stdio` 的 `account/rateLimits/read` JSON-RPC 方法 |
| 构建 | `swiftc`、`sips`、`iconutil`、`codesign`、`ditto` |
| 测试 | 独立 Swift 自测可执行程序，避免依赖 XCTest |

项目不引入第三方 Swift 包，便于在只有 Command Line Tools 的机器上构建和审计。

### 3.1 官方配置依据

官方 Advanced Configuration 文档定义了 `model_provider`、`model_providers.<id>`、`env_key`、`wire_api` 等字段。自定义 Provider 不能使用保留 ID `openai`、`ollama`、`lmstudio`。本项目使用 `custom` 作为 AiLink ID，为其他渠道生成 `custom_<渠道ID>`，并使用独立 `env_key` 让 Codex 从环境变量取密钥。

参考：[OpenAI/Codex Advanced Configuration](https://learn.chatgpt.com/docs/config-file/config-advanced)。

## 4. 项目结构

```text
new-chat-2/
├── Package.swift
├── Sources/CodexSwitch/main.swift       # UI、状态、切换、TOML、Keychain、诊断
├── Resources/Info.plist                 # Bundle 信息和版本
├── Resources/AppIcon.png                # 2048x2048 图标底稿
├── Resources/AppIcon.icns               # macOS 多分辨率图标
├── Tests/TestMain.swift                 # 回归测试入口
├── scripts/build-app.sh                 # 编译、签名、压缩
├── scripts/build-icon.sh                # PNG 生成 ICNS
├── scripts/test.sh                      # 编译并运行测试
├── outputs/CodexSwitch.app              # 应用成品
├── outputs/CodexSwitch-macOS.zip        # 分发压缩包
└── docs/TECHNICAL-DOCUMENTATION.md      # 本文档
```

## 5. 运行时数据

```text
~/Library/Application Support/CodexSwitch/
├── ailink.json
├── preferences.json
├── image-generation-routing.json
└── Backups/
    ├── config-YYYYMMDD-HHMMSS-SSS.toml
    └── state-YYYYMMDD-HHMMSS-SSS.sqlite
```

| 文件 | 内容 | 是否含新密钥 |
| --- | --- | --- |
| `ailink.json` | 兼容旧版本的 AiLink 地址和模型 | 否 |
| `preferences.json` | 全部第三方渠道、官方模型及当前渠道 | 否 |
| `image-generation-routing.json` | `imagegen` 或 `imagegen2` | 否 |
| `Backups/*.toml` | 切换前完整配置快照 | 可能含历史明文密钥 |
| `Backups/*.sqlite` | 切换前完整任务索引 | 不保存任何 API Key，但包含任务元数据 |
| Keychain | 各第三方渠道 API Key（按渠道独立账户名） | 是 |

备份可能继承原始配置里的历史明文 Token，应按敏感数据管理。

## 6. 主要模块

### 6.1 `ConfigStore`

`@MainActor ObservableObject`，连接 UI 与系统操作：

- `refresh()` 解析顶层 Provider，更新官方/指定第三方/未知状态。
- `saveChannelSettings()` 保存渠道地址、模型、协议、路径和可选的新密钥。
- `switchTo(_:)` 启动异步切换事务。
- `loginAndSwitchToOfficial()` 调用 `codex login` 并确认 ChatGPT 登录。
- `restartChatGPT()` 经用户确认后退出并重新打开 ChatGPT。
- `persistModelPreferences()` 保存两套模型选择。
- `switchTo(_:)` 成功后显示重启确认；立即重启会关闭并重新打开 ChatGPT，稍后手动重启则保留切换结果和主界面重启按钮。
- `activateUsageWidget()` 创建常驻 `NSPanel` 并启动 5 分钟刷新周期。
- `refreshUsage()` 根据当前 Provider 选择官方额度或当前第三方余额数据源。

### 6.2 `SwitchEngine`

无 UI 的核心服务，负责备份、配置生成、环境变量、Doctor、HTTP 检查和回滚。所有敏感值只在进程内传递，报告只输出脱敏摘要。

### 6.3 `TOMLEditor`

提供五个受限操作：

- `topLevelValue` 读取顶层键。
- `sectionValue` 读取表段键。
- `settingTopLevel` 修改或插入顶层键。
- `removingTopLevel` 删除指定顶层键。
- `removingSection` 删除完整表段。

它是针对 Codex 配置的定点编辑器，不是完整 TOML AST。复杂多行字符串或数组表语法变更时，应升级为正式 TOML 解析器。

### 6.4 `Keychain` 与 `Launchctl`

Keychain 条目使用：

```text
Service: com.local.CodexSwitch
Account: AiLink（内置渠道）或 `Channel.<渠道ID>`（自定义渠道）
Class: kSecClassGenericPassword
Accessibility: kSecAttrAccessibleAfterFirstUnlock
```

`launchctl setenv AILINK_API_KEY` 或 `launchctl setenv CODEX_<渠道ID>_API_KEY` 是 Codex 子进程读取密钥的桥接；Keychain 是主存储。生图路由使用 `CODEX_SWITCH_IMAGE_SKILL` 环境变量并同步保存 JSON 状态。

## 7. 切换算法

### 7.1 AiLink

最终配置核心字段：

```toml
model_provider = "custom"
model = "<服务端支持的模型>"

[model_providers.custom]
name = "AiLink"
base_url = "https://ai.ailink1.com"
env_key = "AILINK_API_KEY"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
```

流程：

```text
点击 AiLink
  -> 读取 Keychain
  -> 备份 config.toml
  -> GET <base_url>/v1/models
  -> 校验模型存在
  -> 设置 AILINK_API_KEY
  -> 写入 custom Provider
  -> codex doctor
  -> HTTP 可达性检查
  -> 成功，或恢复所有旧状态
```

### 7.2 自定义第三方渠道

用户可在“添加自定义渠道”中填写：渠道名称、HTTPS API 地址、默认模型、模型列表路径、余额路径、`Responses` 或 `Chat Completions` 协议，以及是否切换前校验模型列表。每个渠道生成独立 Provider ID（`custom_<渠道ID>`）和独立环境变量/Keychain 账户。例如：

```toml
model_provider = "custom_channel_test"
model = "gpt-5.5"

[model_providers.custom_channel_test]
name = "自定义中转"
base_url = "https://proxy.example.com/v1"
env_key = "CODEX_CHANNEL_TEST_API_KEY"
wire_api = "chat"
requires_openai_auth = false
supports_websockets = false
```

切换流程与 AiLink 完全相同：读取对应 Keychain 密钥、备份配置、按用户设置请求模型列表并校验、写入 Provider、执行 Doctor 和服务可达性检查、批量覆盖旧任务，失败则恢复配置和环境变量。余额悬浮窗复用同一解析器；如果中转站没有余额接口，可将余额路径留空，切换功能仍可使用。

### 7.3 官方

官方模式的关键是保留 Provider 定义、取消顶层激活：

```toml
model = "gpt-5.6-sol"

[model_providers.custom]
name = "AiLink"
base_url = "https://ai.ailink1.com"
env_key = "AILINK_API_KEY"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
```

代码会移除顶层 `model_provider = "custom"`，但保留已保存的第三方 Provider 段。新会话使用官方内置 Provider，任务索引中的旧 OpenAI/第三方会话统一覆盖为 `openai`。

### 7.4 全部旧任务覆盖

切换验证通过后，应用对 `~/.codex/state_5.sqlite` 执行：

```sql
PRAGMA busy_timeout=10000;
BEGIN IMMEDIATE;
UPDATE threads
SET model_provider = '<当前 Provider>', model = '<当前模型>'
WHERE (model_provider = 'openai' OR model_provider = 'custom' OR model_provider LIKE 'custom_%') AND preview <> '';
COMMIT;
```

`preview <> ''` 对应 Codex 侧边栏可见任务，避免覆盖隐藏的自动审查和内部任务。更新前使用 SQLite `.backup` 创建一致性备份。事务后再次查询，确认所有目标任务均为当前 Provider/模型。应用不会修改 `rollout-*.jsonl`，不会新增 turn，也不会调用模型。

Codex 已经加载到内存的任务会保留旧设置，因此切换完成后应用自动退出并重新打开 ChatGPT。重启后任意旧任务都从更新后的索引恢复。

### 7.5 事务回滚

1. 读取原始文本和旧环境变量。
2. 写入配置和任务索引的带毫秒时间戳备份。
3. 用临时文件和原子替换写入配置。
4. 运行相关诊断。
5. 在 SQLite 单一事务中覆盖全部目标任务并验证。
6. 任意关键检查失败，恢复原文本、`AILINK_API_KEY` 和生图变量；SQLite 事务失败会自动回滚。

## 8. 模型管理

官方和每个第三方渠道分别保存模型，避免跨渠道复用不兼容值。官方候选列表：

```text
gpt-5.2
gpt-5.5
gpt-5.6-luna
gpt-5.6-terra
gpt-5.6-sol
```

第三方渠道切换时请求：

```http
GET https://ai.ailink1.com/v1/models
Authorization: Bearer <Keychain 中的密钥>
```

只接受返回 JSON 的 `data[].id`。若接口失败、非 2xx、格式无法解析或当前模型不存在，切换失败并回滚。

模型列表是服务端能力，不代表账号一定有每个模型的权限；官方候选列表也不代表每个账号都拥有全部权限。

## 9. 生图路由

切换成功后写入：

```text
~/Library/Application Support/CodexSwitch/image-generation-routing.json
CODEX_SWITCH_IMAGE_SKILL=imagegen
CODEX_SWITCH_IMAGE_SKILL=imagegen2
```

映射关系：

| 渠道 | 默认技能 |
| --- | --- |
| OpenAI 官方 | `$imagegen` |
| 所有第三方渠道 | `$imagegen2` |

这是应用级默认提示和状态记录，不是 Codex 官方的 Provider 配置项。应用不会改写技能文件，也不会改变已打开任务。

## 10. 额度悬浮窗

额度窗口由无边框、非激活 AppKit `NSPanel` 承载 SwiftUI 胶囊内容，窗口级别为 `floating`，尺寸由 282×58 进一步缩为 160×34，可跨桌面空间显示、拖动、关闭，并通过主窗口的“额度悬浮窗”按钮重新打开。`nonactivatingPanel` 与 `ignoresCycle` 确保点击胶囊不会激活应用或把主窗口带到前台。窗口启动后立即刷新，此后每 10 秒刷新一次，也提供手动刷新按钮；刷新失败会保留上次成功数据并显示橙色状态点，不会把失败误判为零余额。

胶囊菜单只提供自由悬浮、靠边隐藏和关闭悬浮。选择靠边隐藏不会锁定当前位置，窗口仍可正常拖动；胶囊使用自身的拖拽手势直接更新 `NSPanel` 坐标，用户把它拖到距离当前屏幕任一边缘 48pt 内并松开鼠标后立即判定最近边缘并自动收起，不依赖无标题浮窗可能遗漏的窗口移动通知。停靠状态和实际边缘存入 `UserDefaults`。收起时窗口平移到屏幕外，仅保留 10pt 触发区域；鼠标移入后以 0.18 秒动画滑出，移开 650ms 后重新隐藏。用户把已展开的胶囊拖离边缘会自动解除停靠。自由悬浮时保留当前位置，并遵循系统“减少动态效果”设置。

### 10.1 OpenAI 官方额度

应用启动一个短生命周期的 Codex `app-server --stdio` 进程，完成 JSON-RPC 初始化后调用：

```text
account/rateLimits/read
```

返回的 `primary`/`secondary` 窗口按 `windowDurationMins` 识别：约 300 分钟对应 5 小时额度，约 10080 分钟对应周额度。后端返回的是 `usedPercent`，UI 使用 `100 - usedPercent` 显示剩余百分比。两项额度严格固定在同一行；鼠标停在 5 小时或周额度区域时，应用使用独立的非激活提示面板显示剩余百分比和 `resetsAt` 对应的本地重置时间，不依赖在浮动面板上可能不出现的 macOS 系统 Help Tag。这个读取过程只访问账户限额状态，不发送模型消息。

OpenAI 官方说明中，Codex 本地消息与云端任务共享 5 小时窗口，并可能适用每周限制。参考：[Codex Pricing](https://learn.chatgpt.com/docs/pricing)。

### 10.2 第三方渠道可用余额

应用从 Keychain 读取当前第三方渠道 API Key，请求该渠道设置的余额路径：

```http
GET <base_url>/v1/usage
Authorization: Bearer <Keychain 中的密钥>
```

兼容 AiLink 的不限额钱包模式返回顶层 `remaining`/`balance`，悬浮窗显示美元可用余额。若 Key 使用配额模式，则读取 `quota.remaining` 并标记为“剩余配额”。API Key 只存在于 HTTPS Authorization 请求头，不写入日志、UI 或配置文件。

## 11. 诊断策略

Doctor 可能把更新 CDN、桌面更新或网络提醒计入非零退出码，但这些不一定代表 Provider 不能工作。因此应用只把相关条件作为切换判据：

- `config` 必须为 `[ok]`。
- `auth` 必须为 `[ok]`。
- 第三方渠道必须明确显示 WebSocket 未启用，并按渠道选择 Responses 或 Chat Completions 协议。
- 官方允许 WebSocket 失败后 HTTPS fallback，并以黄色警告提示。

切换报告显示配置文件、Doctor、官方登录或当前第三方服务、密钥注入方式、旧 Provider 保留状态和备份文件。

## 12. 安全与权限

- UI 不回显现有密钥；空输入保存会保留旧值。
- 新值覆盖 Keychain，不写入新的明文配置。
- 命令摘要会隐藏常见 `sk-...` 形式。
- 旧备份可能含历史明文 Token，必须限制访问。
- `launchctl` 用户环境变量可被同用户进程读取，不能替代系统级秘密隔离。
- 当前应用使用 ad-hoc 签名，未使用 Developer ID/notarization；其他机器可能触发 Gatekeeper。
- “重启 ChatGPT”会中断运行中的任务，必须由用户确认。

## 13. 构建发布

环境要求：macOS 14+、arm64、Swift Command Line Tools、ChatGPT 内置 `codex`、`sips`、`iconutil`、`codesign` 和 `ditto`。

```bash
cd /Users/Admin/Documents/Codex/2026-08-09/new-chat-2
./scripts/test.sh
./scripts/build-app.sh
```

`build-app.sh` 依次生成 ICNS、用 `swiftc` 编译、复制 Bundle 资源、ad-hoc 签名并用 `ditto` 创建 ZIP。

验证命令：

```bash
codesign --verify --deep --strict outputs/CodexSwitch.app
plutil -p outputs/CodexSwitch.app/Contents/Info.plist
unzip -t outputs/CodexSwitch-macOS.zip
```

## 14. 测试策略

```bash
./scripts/test.sh
```

当前自测覆盖：

- TOML 定点修改不破坏 MCP 和插件段。
- 官方模式移除顶层 Provider 后保留 `custom` 段。
- 明文 Token、错误 WebSocket 和错误认证配置被拒绝。
- Doctor 关键失败会拒绝；官方 HTTPS fallback 不会误判失败。
- 官方/第三方生图映射正确。
- 官方 5 小时/周额度返回解析与剩余百分比换算正确。
- AiLink 钱包余额和配额模式的剩余额度解析正确；自定义渠道复用同一余额解析器。
- 批量覆盖 `openai/custom` 任务并保留其他 Provider。
- 任务索引修改前生成可读取的 SQLite 备份。

未自动化覆盖：真实浏览器 OAuth、真实旧会话 UI、Developer ID/notarization、不同 Codex 版本的模型目录差异。

## 15. 故障排查

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| `custom not found` | 旧版本删除了 Provider 段，或配置被外部覆盖 | 用新版切换一次官方，确认 `custom` 段存在，重启并新建任务 |
| 第三方模型无法连接 | 模型不在服务端模型列表，或协议/路径配置不匹配 | 检查渠道编辑页的协议、路径和模型；必要时关闭模型列表校验 |
| 官方模型无法使用 | 账号权限或模型目录不包含该模型 | 换用其他官方候选模型 |
| 反复重连 | Provider、认证、WebSocket 或代理不匹配 | 重新切换并重启 ChatGPT；第三方渠道保持 WebSocket 关闭 |
| 旧会话仍使用原渠道 | ChatGPT 尚未重启，或 `state_5.sqlite` 表结构已改变 | 在重启确认中选择“立即重启”，或稍后点击“重启 ChatGPT”；检查“全部旧任务”诊断项 |
| 官方额度读取失败 | 未登录官方账号、Codex app-server 不可用或请求超时 | 点击“登录并切换 OpenAI”，完成登录后在悬浮窗重试 |
| 第三方余额读取失败 | API Key 无效，或服务端未提供已配置的余额路径 | 在渠道设置中更新 Key/余额路径；没有余额接口时可留空，切换功能不受影响 |

## 16. 后续演进

1. 引入正式 TOML AST 解析器。
2. 从 Codex 官方模型目录动态读取官方模型。
3. 缓存各渠道模型列表并提供离线状态。
4. 增加备份保留数量、清理和脱敏导出。
5. 使用 Developer ID 签名和 notarization。
6. Codex 若提供正式批量任务设置 API，替换当前 SQLite 索引更新实现。

## 17. 文件索引

- [`Sources/CodexSwitch/main.swift`](../Sources/CodexSwitch/main.swift)：全部应用逻辑。
- [`Resources/Info.plist`](../Resources/Info.plist)：Bundle 和版本。
- [`scripts/build-app.sh`](../scripts/build-app.sh)：构建发布。
- [`scripts/build-icon.sh`](../scripts/build-icon.sh)：图标生成。
- [`scripts/test.sh`](../scripts/test.sh)：回归测试。
- [`Tests/TestMain.swift`](../Tests/TestMain.swift)：测试断言。
- [`README.md`](../README.md)：快速使用说明。
