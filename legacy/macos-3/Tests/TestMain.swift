import Foundation

@main
struct TestMain {
    static func main() {
        let fixture = """
        model_provider = "custom"
        model = "gpt-5.5"

        [model_providers.custom]
        name = "AiLink"
        base_url = "https://ai.ailink1.com"
        experimental_bearer_token = "secret-value"

        [mcp_servers.node]
        command = "node"

        [plugins."browser@openai-bundled"]
        enabled = true
        """

        let removed = TOMLEditor.removingSection("model_providers.custom", from: fixture)
        require(!removed.contains("experimental_bearer_token"), "AiLink secret section removal")
        require(removed.contains("[mcp_servers.node]"), "MCP section preservation")
        require(removed.contains("[plugins.\"browser@openai-bundled\"]"), "plugin section preservation")

        let changed = TOMLEditor.settingTopLevel("model", value: "gpt-5.6-sol", in: fixture)
        require(TOMLEditor.topLevelValue("model", in: changed) == "gpt-5.6-sol", "top-level model update")
        let official = TOMLEditor.removingTopLevel("model_provider", from: changed)
        require(TOMLEditor.topLevelValue("model_provider", in: official) == nil, "official provider reset")
        require(TOMLEditor.sectionValue("name", section: "model_providers.custom", in: official) == "AiLink", "nested value preservation")
        require(TOMLEditor.sectionValue("base_url", section: "model_providers.custom", in: official) == "https://ai.ailink1.com", "old-session provider resolution")

        require(ImageGenerationSkill.forTarget(.official) == .imagegen, "official image skill routing")
        require(ImageGenerationSkill.forTarget(.aiLink) == .imagegen2, "AiLink image skill routing")
        let customChannel = ChannelProfile(id: "channel-test", name: "自定义中转", baseURL: "https://proxy.example.com/v1", model: "gpt-5.5", modelsPath: "/v1/models", usagePath: "/v1/usage", wireAPI: "chat", validatesModelList: true, isBuiltIn: false)
        require(SwitchEngine.providerID(for: customChannel) == "custom_channel_test", "custom provider id")
        require(SwitchEngine.environmentKey(for: customChannel) == "CODEX_CHANNEL_TEST_API_KEY", "custom environment key")
        require(customChannel.endpoint(path: "/v1/models")?.absoluteString == "https://proxy.example.com/v1/models", "custom endpoint normalization")
        let customConfig = """
        model_provider = "custom_channel_test"
        model = "gpt-5.5"

        [model_providers.custom_channel_test]
        name = "自定义中转"
        base_url = "https://proxy.example.com/v1"
        env_key = "CODEX_CHANNEL_TEST_API_KEY"
        wire_api = "chat"
        requires_openai_auth = false
        supports_websockets = false
        """
        require(SwitchEngine.isConformantChannelConfig(customConfig, channel: customChannel), "custom provider config conformance")
        require(UsageDockMode.allCases.map(\.rawValue) == ["free", "edge"], "usage widget dock modes")
        let screen = NSRect(x: 0, y: 0, width: 1440, height: 870)
        require(UsageDockGeometry.nearestEdge(panelFrame: NSRect(x: 20, y: 400, width: 160, height: 34), visibleFrame: screen) == .left, "left edge detection")
        require(UsageDockGeometry.nearestEdge(panelFrame: NSRect(x: 1430, y: 400, width: 160, height: 34), visibleFrame: screen) == .right, "partly off-screen right edge detection")
        require(UsageDockGeometry.nearestEdge(panelFrame: NSRect(x: 600, y: 850, width: 160, height: 34), visibleFrame: screen) == .top, "partly off-screen top edge detection")
        require(UsageDockGeometry.nearestEdge(panelFrame: NSRect(x: 600, y: 300, width: 160, height: 34), visibleFrame: screen) == nil, "non-edge position remains free")

        let officialUsage = Data("""
        {"id":2,"result":{"rateLimits":{"primary":{"usedPercent":11,"windowDurationMins":300,"resetsAt":1787748781},"secondary":{"usedPercent":38,"windowDurationMins":10080,"resetsAt":1788280440},"planType":"plus"}}}
        """.utf8)
        let officialQuota = try! UsageParser.official(from: officialUsage)
        require(officialQuota.fiveHour?.remainingPercent == 89, "official five-hour remaining quota")
        require(officialQuota.weekly?.remainingPercent == 62, "official weekly remaining quota")
        require(officialQuota.fiveHour?.resetsAt != nil, "official five-hour reset time")
        require(officialQuota.weekly?.resetsAt != nil, "official weekly reset time")
        require(officialQuota.planType == "plus", "official plan parsing")

        let aiLinkWallet = Data("""
        {"balance":280.24635844,"isValid":true,"mode":"unrestricted","planName":"钱包余额","remaining":280.24635844}
        """.utf8)
        let wallet = try! UsageParser.aiLink(from: aiLinkWallet)
        require(wallet.remaining == 280.24635844, "AiLink wallet balance parsing")
        require(wallet.planName == "钱包余额", "AiLink wallet plan parsing")

        let aiLinkQuota = Data("""
        {"mode":"quota_limited","quota":{"limit":100,"used":72.5,"remaining":27.5}}
        """.utf8)
        let quota = try! UsageParser.aiLink(from: aiLinkQuota)
        require(quota.remaining == 27.5, "AiLink limited-key remaining quota")
        require(quota.planName == "剩余配额", "AiLink limited-key label")

        require(TOMLEditor.topLevelValue("model_provider", in: official) == nil, "official active provider reset")
        require(TOMLEditor.sectionValue("name", section: "model_providers.custom", in: official) == "AiLink", "legacy custom provider retained")

        let secure = """
        model_provider = "custom"
        model = "gpt-5.5"

        [model_providers.custom]
        name = "AiLink"
        base_url = "https://ai.ailink1.com"
        env_key = "AILINK_API_KEY"
        wire_api = "responses"
        requires_openai_auth = false
        supports_websockets = false
        """
        require(SwitchEngine.isConformantAiLinkConfig(secure), "secure AiLink conformance")
        require(!SwitchEngine.isConformantAiLinkConfig(fixture), "legacy config rejection")
        require(!SwitchEngine.isConformantAiLinkConfig(secure + "\nexperimental_bearer_token = \"secret\""), "plaintext secret rejection")

        let doctor = """
        Configuration
          [ok] config       loaded
          [ok] auth         auth is provided by the active model provider
          [ok] websocket    Responses WebSocket is not enabled for the active provider
        20 ok | 1 warn | 1 fail failed
        """
        require(Command.doctorChecksPassed(doctor, target: .aiLink), "relevant Doctor checks")
        require(!Command.doctorChecksPassed(doctor.replacingOccurrences(of: "[ok] auth", with: "[XX] auth"), target: .aiLink), "Doctor auth failure rejection")
        let officialDoctor = doctor.replacingOccurrences(of: "[ok] websocket    Responses WebSocket is not enabled for the active provider", with: "[!!] websocket    Responses WebSocket failed; HTTPS fallback may still work")
        require(Command.doctorChecksPassed(officialDoctor, target: .official), "official HTTPS fallback acceptance")
        require(!Command.doctorChecksPassed(officialDoctor, target: .aiLink), "AiLink WebSocket failure rejection")

        let temporary = FileManager.default.temporaryDirectory.appendingPathComponent("CodexSwitchTests-\(UUID().uuidString)")
        try! FileManager.default.createDirectory(at: temporary, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: temporary) }
        let database = temporary.appendingPathComponent("state_5.sqlite")
        runSQLite(database, "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL, model TEXT, preview TEXT NOT NULL); INSERT INTO threads VALUES ('official', 'openai', 'gpt-old', 'visible'), ('ailink', 'custom', 'gpt-old', 'visible'), ('proxy', 'custom_proxy_1', 'gpt-old', 'visible'), ('other', 'ollama', 'local-model', 'visible'), ('internal', 'custom', 'codex-auto-review', '');")
        let rebound = try! SessionRebinder.rebindAll(databaseURL: database, backupDirectory: temporary.appendingPathComponent("Backups"), provider: "custom", model: "gpt-5.5")
        require(rebound?.changedCount == 3, "all OpenAI/AiLink/custom sessions rebound")
        require(rebound.map { FileManager.default.fileExists(atPath: $0.backupURL.path) } == true, "session database backup")
        require(runSQLite(database, "SELECT model_provider || ':' || model FROM threads WHERE id='official';") == "custom:gpt-5.5", "official session rebound to AiLink")
        require(runSQLite(database, "SELECT model_provider || ':' || model FROM threads WHERE id='proxy';") == "custom:gpt-5.5", "custom proxy session rebound")
        require(runSQLite(database, "SELECT model_provider || ':' || model FROM threads WHERE id='other';") == "ollama:local-model", "unrelated provider preserved")
        require(runSQLite(database, "SELECT model_provider || ':' || model FROM threads WHERE id='internal';") == "custom:codex-auto-review", "hidden internal task preserved")

        if ProcessInfo.processInfo.environment["CODEX_SWITCH_INTEGRATION"] == "1" {
            let liveQuota = try! UsageParser.official(from: AppServerRPC.rateLimits())
            require(liveQuota.fiveHour != nil, "live OpenAI five-hour quota RPC")
        }

        print("All Codex Switch tests passed")
    }

    private static func require(_ condition: @autoclosure () -> Bool, _ name: String) {
        guard condition() else {
            fputs("FAILED: \(name)\n", stderr)
            exit(1)
        }
    }

    @discardableResult
    private static func runSQLite(_ database: URL, _ sql: String) -> String {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/sqlite3")
        process.arguments = [database.path, sql]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        try! process.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        require(process.terminationStatus == 0, "SQLite fixture command")
        return (String(data: data, encoding: .utf8) ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
