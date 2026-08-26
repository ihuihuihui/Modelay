# Codex Switch

一个原生 macOS 配置切换器，用于在 OpenAI 官方 ChatGPT 登录、AiLink 和其他 OpenAI 兼容中转站之间一键切换。

当前版本：3.0.1。

应用使用青绿色与橙色双路径图标，可在 Dock 和访达中快速识别。

## 使用

1. 打开 `CodexSwitch.app`。应用会自动识别当前渠道、模型，并将已配置密钥保存在 macOS 钥匙串。
2. 点击“OpenAI 官方”或任一第三方渠道。应用会先备份，再切换并运行完整检查。
3. 检查通过后应用会弹出重启确认，可选择“立即重启”或“稍后手动重启”；旧任务与新任务都会使用当前渠道和模型。

应用启动后会显示一个微型胶囊额度悬浮窗：官方渠道显示 5 小时与周额度，第三方渠道显示其余额接口返回的可用余额。悬浮窗每 10 秒实时更新，也可手动刷新。菜单只有“自由悬浮”“靠边隐藏”“关闭悬浮”三项；选择“靠边隐藏”后仍可自由拖动，把胶囊拖到任一屏幕边缘并松开才会自动收起。悬浮窗采用非激活面板，点击刷新或菜单不会把主窗口带到前台。

模型菜单分别维护官方和每个第三方渠道的模型。添加渠道时可填写名称、API 地址、模型、模型列表路径、余额路径、Responses/Chat Completions 协议，并选择是否在切换前校验模型列表；校验开启时只允许服务端实际返回的模型。

如果官方凭据已被第三方 API 登录覆盖，点击“登录并切换 OpenAI”，在浏览器完成一次官方账号授权。此后日常切换无需重复登录。

切换时会先备份 `~/.codex/state_5.sqlite`，再把全部侧边栏可见的 OpenAI/第三方任务覆盖为当前渠道和模型；其他 Provider 与隐藏内部任务不受影响。

生图路由会随渠道记录：官方是 `$imagegen`，所有第三方渠道是 `$imagegen2`。这里的技能名称用于本应用的默认提示，不会改变已经打开的旧任务，也不会伪造 Codex 未公开的配置项。

## 安全设计

- API Key 存储在 macOS 钥匙串，不写入 `config.toml`。
- 每次切换前备份 `~/.codex/config.toml`。
- 第三方渠道使用 Responses 或 Chat Completions 兼容协议、关闭 WebSocket，并设置 `requires_openai_auth = false`。
- 官方模式移除 AiLink 的顶层激活状态，但保留 Provider 定义供旧会话解析，不删除官方登录凭据。
- 插件、MCP、项目权限和其他 Codex 配置保持不变。
- 切换后的健康检查失败时，自动恢复切换前的配置。

## 构建

```bash
chmod +x scripts/build-app.sh
./scripts/build-app.sh
```

成品位于 `outputs/CodexSwitch.app`，可分发压缩包为 `outputs/CodexSwitch-macOS.zip`。

完整技术说明见 [`docs/TECHNICAL-DOCUMENTATION.md`](docs/TECHNICAL-DOCUMENTATION.md)。
