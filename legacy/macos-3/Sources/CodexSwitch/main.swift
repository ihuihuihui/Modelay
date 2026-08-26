import AppKit
import Security
import SwiftUI

#if !CODEX_SWITCH_TESTING
@main
struct CodexSwitchApp: App {
    @StateObject private var store = ConfigStore()

    var body: some Scene {
        WindowGroup {
            ContentView().environmentObject(store).frame(minWidth: 720, minHeight: 600)
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 820, height: 680)
        .commands { CommandGroup(replacing: .newItem) { } }
    }
}
#endif

enum ProviderMode: Equatable, Sendable {
    case official, aiLink, channel(String), unknown(String)

    var title: String {
        switch self {
        case .official: "OpenAI 官方"
        case .aiLink: "AiLink"
        case .channel(let name): name
        case .unknown(let provider): "未知 Provider（\(provider)）"
        }
    }
}

enum SwitchTarget: Sendable {
    case official
    case aiLink
    case channel(ChannelProfile)

    var channel: ChannelProfile? {
        if case .channel(let channel) = self { return channel }
        return nil
    }

    var isOfficial: Bool {
        if case .official = self { return true }
        return false
    }

    var profile: ChannelProfile? {
        switch self {
        case .aiLink: return .aiLink
        case .channel(let profile): return profile
        case .official: return nil
        }
    }
}

enum ImageGenerationSkill: String, Codable, Sendable {
    case imagegen
    case imagegen2

    var displayName: String { "$\(rawValue)" }
    var detail: String {
        switch self {
        case .imagegen: "OpenAI 官方图像生成"
        case .imagegen2: "第三方渠道图像生成"
        }
    }

    static func forTarget(_ target: SwitchTarget) -> Self {
        target.isOfficial ? .imagegen : .imagegen2
    }
}

/// 2.x 旧版 AiLink 配置，仅用于无损迁移。
struct AiLinkSettings: Codable, Equatable, Sendable {
    var baseURL = "https://ai.ailink1.com"
    var model = "gpt-5.5"
    var normalizedBaseURL: String {
        baseURL.trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    }
}

struct ChannelProfile: Codable, Equatable, Identifiable, Sendable {
    var id: String
    var name: String
    var baseURL: String
    var model: String
    var modelsPath: String
    var usagePath: String
    var wireAPI: String
    var validatesModelList: Bool
    var isBuiltIn: Bool

    static let aiLink = ChannelProfile(
        id: "ailink",
        name: "AiLink",
        baseURL: "https://ai.ailink1.com",
        model: "gpt-5.5",
        modelsPath: "/v1/models",
        usagePath: "/v1/usage",
        wireAPI: "responses",
        validatesModelList: true,
        isBuiltIn: true
    )

    var normalizedName: String { name.trimmingCharacters(in: .whitespacesAndNewlines) }
    var normalizedBaseURL: String {
        baseURL.trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    }
    var normalizedModel: String { model.trimmingCharacters(in: .whitespacesAndNewlines) }

    enum CodingKeys: String, CodingKey { case id, name, baseURL, model, modelsPath, usagePath, wireAPI, validatesModelList, isBuiltIn }

    init(id: String, name: String, baseURL: String, model: String, modelsPath: String, usagePath: String, wireAPI: String = "responses", validatesModelList: Bool = true, isBuiltIn: Bool = false) {
        self.id = id; self.name = name; self.baseURL = baseURL; self.model = model; self.modelsPath = modelsPath; self.usagePath = usagePath; self.wireAPI = wireAPI; self.validatesModelList = validatesModelList; self.isBuiltIn = isBuiltIn
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        baseURL = try c.decode(String.self, forKey: .baseURL)
        model = try c.decode(String.self, forKey: .model)
        modelsPath = try c.decodeIfPresent(String.self, forKey: .modelsPath) ?? "/v1/models"
        usagePath = try c.decodeIfPresent(String.self, forKey: .usagePath) ?? "/v1/usage"
        wireAPI = try c.decodeIfPresent(String.self, forKey: .wireAPI) ?? "responses"
        validatesModelList = try c.decodeIfPresent(Bool.self, forKey: .validatesModelList) ?? true
        isBuiltIn = try c.decodeIfPresent(Bool.self, forKey: .isBuiltIn) ?? false
    }

    func endpoint(path: String) -> URL? {
        let normalizedPath = path.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalizedPath.isEmpty else { return nil }
        let base = normalizedBaseURL
        let suffix = normalizedPath.hasPrefix("/") ? normalizedPath : "/\(normalizedPath)"
        if base.hasSuffix("/v1"), suffix.hasPrefix("/v1/") {
            return URL(string: base + String(suffix.dropFirst(3)))
        }
        return URL(string: base + suffix)
    }
}

struct CodexSwitchPreferences: Codable, Sendable {
    var aiLink: AiLinkSettings
    var officialModel: String
}

struct ChannelPreferences: Codable, Sendable {
    var channels: [ChannelProfile]
    var officialModel: String
    var lastChannelID: String?
}

struct CheckResult: Identifiable, Sendable {
    enum State: Sendable { case passed, warning, failed }
    let id = UUID()
    let title: String
    let detail: String
    let state: State
}

struct SwitchReport: Sendable {
    let checks: [CheckResult]
    let backupURL: URL
}

struct SessionRebindReport: Sendable {
    let changedCount: Int
    let backupURL: URL
}

struct BannerMessage: Equatable {
    enum Kind { case success, error, warning }
    let kind: Kind
    let text: String
    static func success(_ text: String) -> Self { .init(kind: .success, text: text) }
    static func error(_ text: String) -> Self { .init(kind: .error, text: text) }
    static func warning(_ text: String) -> Self { .init(kind: .warning, text: text) }
}

enum SwitchError: LocalizedError {
    case missingAPIKey(String), invalidBaseURL(String), invalidModel(String), invalidChannelName, invalidChannelProtocol
    case commandFailed(String), validationFailed(String)

    var errorDescription: String? {
        switch self {
        case .missingAPIKey(let name): "尚未保存 \(name) 的 API Key，请先打开渠道设置。"
        case .invalidBaseURL(let name): "\(name) 的 API 地址无效，请填写完整的 HTTPS 地址。"
        case .invalidModel(let name): "\(name) 的模型名称不能为空。"
        case .invalidChannelName: "渠道名称不能为空。"
        case .invalidChannelProtocol: "API 协议只能选择 Responses 或 Chat Completions。"
        case .commandFailed(let detail): detail
        case .validationFailed(let detail): "验证未通过，已自动恢复切换前的配置。\(detail)"
        }
    }
}

struct QuotaWindowSnapshot: Equatable, Sendable {
    let usedPercent: Double
    let remainingPercent: Double
    let durationMinutes: Int?
    let resetsAt: Date?
}

struct OfficialQuotaSnapshot: Equatable, Sendable {
    let fiveHour: QuotaWindowSnapshot?
    let weekly: QuotaWindowSnapshot?
    let planType: String?
}

struct AiLinkBalanceSnapshot: Equatable, Sendable {
    let remaining: Double
    let planName: String
    let mode: String?
}

enum UsageSnapshot: Equatable, Sendable {
    case official(OfficialQuotaSnapshot)
    case aiLink(AiLinkBalanceSnapshot)
}

enum UsageDockMode: String, CaseIterable, Sendable {
    case free, edge

    var title: String {
        switch self {
        case .free: "自由悬浮"
        case .edge: "靠边隐藏"
        }
    }
}

enum UsageDockEdge: String { case left, right, top, bottom }

enum UsageDockGeometry {
    static func nearestEdge(panelFrame: NSRect, visibleFrame: NSRect, threshold: CGFloat = 48) -> UsageDockEdge? {
        let distances: [(UsageDockEdge, CGFloat)] = [
            (.left, max(0, panelFrame.minX - visibleFrame.minX)),
            (.right, max(0, visibleFrame.maxX - panelFrame.maxX)),
            (.top, max(0, visibleFrame.maxY - panelFrame.maxY)),
            (.bottom, max(0, panelFrame.minY - visibleFrame.minY))
        ]
        guard let nearest = distances.min(by: { $0.1 < $1.1 }), nearest.1 <= threshold else { return nil }
        return nearest.0
    }
}

struct UsageTooltipView: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(.primary)
            .lineLimit(1)
            .fixedSize(horizontal: true, vertical: true)
            .padding(.horizontal, 10)
            .frame(height: 28)
            .background(.ultraThinMaterial, in: Capsule())
            .overlay(Capsule().stroke(.white.opacity(0.35), lineWidth: 0.8))
    }
}

enum UsageParsingError: LocalizedError {
    case invalidResponse(String)

    var errorDescription: String? {
        switch self {
        case .invalidResponse(let detail): detail
        }
    }
}

enum UsageParser {
    static func official(from data: Data) throws -> OfficialQuotaSnapshot {
        guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let result = root["result"] as? [String: Any],
              let limits = result["rateLimits"] as? [String: Any] else {
            if let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let error = root["error"] as? [String: Any],
               let message = error["message"] as? String {
                throw UsageParsingError.invalidResponse(message)
            }
            throw UsageParsingError.invalidResponse("OpenAI 未返回可识别的额度数据。")
        }

        let candidates = [limits["primary"], limits["secondary"]]
            .compactMap { $0 as? [String: Any] }
            .compactMap(parseWindow)
        let fiveHour = candidates.first { window in
            guard let duration = window.durationMinutes else { return false }
            return (240...360).contains(duration)
        } ?? candidates.first
        let weekly = candidates.first { window in
            guard let duration = window.durationMinutes else { return false }
            return (9_000...11_000).contains(duration)
        } ?? (candidates.count > 1 ? candidates[1] : nil)

        return OfficialQuotaSnapshot(
            fiveHour: fiveHour,
            weekly: weekly,
            planType: limits["planType"] as? String
        )
    }

    static func aiLink(from data: Data) throws -> AiLinkBalanceSnapshot {
        guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw UsageParsingError.invalidResponse("AiLink 未返回可识别的余额数据。")
        }
        if let error = root["error"] as? [String: Any], let message = error["message"] as? String {
            throw UsageParsingError.invalidResponse(message)
        }
        let quota = root["quota"] as? [String: Any]
        let remaining = number(root["remaining"])
            ?? number(root["balance"])
            ?? number(quota?["remaining"])
        guard let remaining else {
            throw UsageParsingError.invalidResponse("AiLink 返回成功，但没有可用余额字段。")
        }
        let planName = (root["planName"] as? String)
            ?? ((root["mode"] as? String) == "quota_limited" ? "剩余配额" : "钱包余额")
        return AiLinkBalanceSnapshot(remaining: max(0, remaining), planName: planName, mode: root["mode"] as? String)
    }

    private static func parseWindow(_ object: [String: Any]) -> QuotaWindowSnapshot? {
        guard let used = number(object["usedPercent"]) else { return nil }
        let clampedUsed = min(100, max(0, used))
        let duration = number(object["windowDurationMins"]).map(Int.init)
        let reset = number(object["resetsAt"]).map { Date(timeIntervalSince1970: $0) }
        return .init(usedPercent: clampedUsed, remainingPercent: 100 - clampedUsed, durationMinutes: duration, resetsAt: reset)
    }

    private static func number(_ value: Any?) -> Double? {
        if let number = value as? NSNumber { return number.doubleValue }
        if let string = value as? String { return Double(string) }
        return nil
    }
}

enum UsageService {
    static func fetchOfficial() throws -> OfficialQuotaSnapshot {
        let response = try AppServerRPC.rateLimits()
        return try UsageParser.official(from: response)
    }

    static func fetchAiLink(baseURL: String, apiKey: String) async throws -> AiLinkBalanceSnapshot {
        var channel = ChannelProfile.aiLink
        channel.baseURL = baseURL
        return try await fetchChannel(channel: channel, apiKey: apiKey)
    }

    static func fetchChannel(channel: ChannelProfile, apiKey: String) async throws -> AiLinkBalanceSnapshot {
        guard let url = channel.endpoint(path: channel.usagePath) else {
            throw SwitchError.invalidBaseURL(channel.name)
        }
        var request = URLRequest(url: url)
        request.timeoutInterval = 12
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw UsageParsingError.invalidResponse("\(channel.name) 未返回有效的 HTTP 响应。")
        }
        guard (200..<300).contains(http.statusCode) else {
            if let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let message = (object["message"] as? String) ?? ((object["error"] as? [String: Any])?["message"] as? String) {
                throw UsageParsingError.invalidResponse("\(channel.name) 余额查询失败：\(message)")
            }
            throw UsageParsingError.invalidResponse("\(channel.name) 余额查询失败（HTTP \(http.statusCode)）。")
        }
        return try UsageParser.aiLink(from: data)
    }
}

enum AppServerRPC {
    static func rateLimits(timeout: TimeInterval = 12) throws -> Data {
        let candidates = ["/Applications/ChatGPT.app/Contents/Resources/codex", "/opt/homebrew/bin/codex", "/usr/local/bin/codex"]
        guard let executable = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else {
            throw UsageParsingError.invalidResponse("找不到 Codex 组件，无法读取 OpenAI 额度。")
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = ["app-server", "--stdio"]
        let input = Pipe(), output = Pipe()
        process.standardInput = input
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice

        let semaphore = DispatchSemaphore(value: 0)
        let lock = NSLock()
        var buffer = Data()
        var responseData: Data?

        output.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            guard !chunk.isEmpty else {
                semaphore.signal()
                return
            }
            lock.lock()
            buffer.append(chunk)
            while let newline = buffer.firstIndex(of: 0x0A) {
                let line = Data(buffer[..<newline])
                buffer.removeSubrange(...newline)
                if let object = try? JSONSerialization.jsonObject(with: line) as? [String: Any],
                   (object["id"] as? NSNumber)?.intValue == 2 {
                    responseData = line
                    lock.unlock()
                    semaphore.signal()
                    return
                }
            }
            lock.unlock()
        }

        do {
            try process.run()
            let requests: [[String: Any]] = [
                ["id": 1, "method": "initialize", "params": [
                    "clientInfo": ["name": "codex-switch", "title": "Codex Switch", "version": "3.0.1"],
                    "capabilities": NSNull()
                ]],
                ["method": "initialized"],
                ["id": 2, "method": "account/rateLimits/read"]
            ]
            for request in requests {
                let line = try JSONSerialization.data(withJSONObject: request) + Data([0x0A])
                try input.fileHandleForWriting.write(contentsOf: line)
            }
        } catch {
            output.fileHandleForReading.readabilityHandler = nil
            if process.isRunning { process.terminate() }
            throw error
        }

        let waitResult = semaphore.wait(timeout: .now() + timeout)
        output.fileHandleForReading.readabilityHandler = nil
        try? input.fileHandleForWriting.close()
        if process.isRunning { process.terminate() }
        lock.lock()
        let finalData = responseData
        lock.unlock()
        guard waitResult == .success, let finalData else {
            throw UsageParsingError.invalidResponse("读取 OpenAI 额度超时，请确认已登录官方账号。")
        }
        return finalData
    }
}

@MainActor
final class ConfigStore: ObservableObject {
    @Published var mode: ProviderMode = .official
    @Published var configurationIsConformant = true
    @Published var channels: [ChannelProfile] = [.aiLink]
    @Published var selectedChannelID = ChannelProfile.aiLink.id
    @Published var editedChannel = ChannelProfile.aiLink
    @Published var editingChannelID: String?
    @Published var channelModels: [String: [String]] = [:]
    @Published var settings = AiLinkSettings()
    @Published var editedSettings = AiLinkSettings()
    @Published var editedAPIKey = ""
    @Published var hasAPIKey = false
    @Published var isEditing = false
    @Published var isWorking = false
    @Published var message: BannerMessage?
    @Published var report: SwitchReport?
    @Published var showRestartConfirmation = false
    @Published var imageGenerationSkill: ImageGenerationSkill = .imagegen
    @Published var officialModel = "gpt-5.6-sol"
    @Published var aiLinkModels = ["gpt-5.4", "gpt-5.5", "gpt-5.6-sol", "gpt-5.6-terra"]
    @Published var usageSnapshot: UsageSnapshot?
    @Published var usageError: String?
    @Published var usageLastUpdated: Date?
    @Published var isUsageLoading = false
    @Published var usageDockMode: UsageDockMode = .free

    let officialModels = ["gpt-5.2", "gpt-5.5", "gpt-5.6-luna", "gpt-5.6-terra", "gpt-5.6-sol"]

    private let fm = FileManager.default
    private let home = FileManager.default.homeDirectoryForCurrentUser
    private let keychainService = "com.local.CodexSwitch"
    private let keychainAccount = "AiLink"
    private var configURL: URL { home.appendingPathComponent(".codex/config.toml") }
    private var supportURL: URL { home.appendingPathComponent("Library/Application Support/CodexSwitch") }
    private var settingsURL: URL { supportURL.appendingPathComponent("ailink.json") }
    private var preferencesURL: URL { supportURL.appendingPathComponent("preferences.json") }
    private var imageSkillURL: URL { supportURL.appendingPathComponent("image-generation-routing.json") }
    private var usagePanel: NSPanel?
    private var usageTooltipPanel: NSPanel?
    private var usageRefreshTask: Task<Void, Never>?
    private var usageTimerTask: Task<Void, Never>?
    private var usageHideTask: Task<Void, Never>?
    private var usageWidgetActivated = false
    private var usageDragOrigin: NSPoint?
    private var usageDragMouseOrigin: NSPoint?
    private var usageDockEdge: UsageDockEdge = .right
    private var usageIsDocked = false
    private var usagePanelIsAnimating = false
    private var usageIgnoreHoverUntil = Date.distantPast
    private let usageDockDefaultsKey = "usageWidgetDockMode"
    private let usageDockEdgeDefaultsKey = "usageWidgetDockEdge"
    private let usageIsDockedDefaultsKey = "usageWidgetIsDocked"

    var selectedChannel: ChannelProfile? {
        channels.first(where: { $0.id == selectedChannelID })
    }

    var modeIsOfficial: Bool {
        if case .official = mode { return true }
        return false
    }

    var selectedChannelModels: [String] {
        selectedChannel.map(channelModelsFor) ?? []
    }

    var selectedChannelHasAPIKey: Bool {
        guard let channel = selectedChannel else { return false }
        return Keychain.read(service: keychainService, account: keychainAccount(for: channel)) != nil
    }

    var editingChannelHasAPIKey: Bool {
        Keychain.read(service: keychainService, account: keychainAccount(for: editedChannel)) != nil
    }

    func keychainAccount(for channel: ChannelProfile) -> String {
        channel.id == ChannelProfile.aiLink.id ? keychainAccount : "Channel.\(channel.id)"
    }

    func keychainValueExists(for channel: ChannelProfile) -> Bool {
        Keychain.read(service: keychainService, account: keychainAccount(for: channel)) != nil
    }

    func channelModelsFor(_ channel: ChannelProfile) -> [String] {
        let saved = channelModels[channel.id] ?? []
        return Array(Set(saved + [channel.model])).sorted()
    }

    init() {
        if let savedDock = UserDefaults.standard.string(forKey: usageDockDefaultsKey) {
            usageDockMode = savedDock == "free" ? .free : .edge
        }
        if let savedEdge = UserDefaults.standard.string(forKey: usageDockEdgeDefaultsKey),
           let edge = UsageDockEdge(rawValue: savedEdge) { usageDockEdge = edge }
        usageIsDocked = UserDefaults.standard.bool(forKey: usageIsDockedDefaultsKey)
        loadSettings()
        loadPreferences()
        migrateAndLoadChannels()
        loadImageGenerationSkill()
        importCurrentAiLinkConfiguration()
        refresh()
    }

    func refresh() {
        let previousMode = mode
        guard let text = try? String(contentsOf: configURL, encoding: .utf8) else {
            mode = .official
            configurationIsConformant = true
            if usageWidgetActivated, previousMode != mode { refreshUsage() }
            return
        }
        switch TOMLEditor.topLevelValue("model_provider", in: text) {
        case nil:
            mode = .official
            configurationIsConformant = true
            imageGenerationSkill = .imagegen
            if let model = TOMLEditor.topLevelValue("model", in: text), officialModels.contains(model) { officialModel = model }
        case SwitchEngine.providerID:
            mode = .aiLink
            configurationIsConformant = SwitchEngine.isConformantAiLinkConfig(text)
            imageGenerationSkill = .imagegen2
            if let model = TOMLEditor.topLevelValue("model", in: text), !model.isEmpty { settings.model = model }
            selectedChannelID = ChannelProfile.aiLink.id
        case .some(let provider) where provider.hasPrefix("custom_"):
            if let channel = channels.first(where: { SwitchEngine.providerID(for: $0) == provider }) {
                let id = channel.id
                selectedChannelID = channel.id
                mode = .channel(channel.name)
                configurationIsConformant = SwitchEngine.isConformantChannelConfig(text, channel: channel)
                imageGenerationSkill = .imagegen2
            if let model = TOMLEditor.topLevelValue("model", in: text), !model.isEmpty,
                   let index = channels.firstIndex(where: { $0.id == id }) {
                    channels[index].model = model
                    settings = AiLinkSettings(baseURL: channels[index].baseURL, model: model)
                }
            } else {
                mode = .unknown(provider)
                configurationIsConformant = false
            }
        case .some(let provider):
            mode = .unknown(provider)
            configurationIsConformant = false
        }
        hasAPIKey = selectedChannelHasAPIKey
        if usageWidgetActivated, previousMode != mode { refreshUsage() }
    }

    func activateUsageWidget() {
        guard !usageWidgetActivated else { return }
        usageWidgetActivated = true
        showUsageWidget()
        refreshUsage()
        usageTimerTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 10_000_000_000)
                guard !Task.isCancelled else { return }
                self?.refreshUsage()
            }
        }
    }

    func showUsageWidget() {
        if let usagePanel {
            usagePanel.orderFrontRegardless()
            if usageDockMode == .edge, usageIsDocked { applyDockPosition(hidden: false, animated: true) }
            return
        }

        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 160, height: 34),
            styleMask: [.borderless, .nonactivatingPanel, .utilityWindow],
            backing: .buffered,
            defer: false
        )
        panel.title = "渠道额度"
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.isMovableByWindowBackground = false
        panel.isReleasedWhenClosed = false
        panel.hidesOnDeactivate = false
        panel.level = .floating
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
        panel.becomesKeyOnlyIfNeeded = true
        panel.contentView = NSHostingView(rootView: UsageWidgetView().environmentObject(self))
        let restoredPosition = panel.setFrameUsingName("CodexSwitchUsageWidget")
        panel.setContentSize(NSSize(width: 160, height: 34))
        if !restoredPosition, let screen = NSScreen.main {
            let frame = screen.visibleFrame
            panel.setFrameOrigin(NSPoint(x: frame.maxX - panel.frame.width - 12, y: frame.maxY - panel.frame.height - 12))
        }
        panel.setFrameAutosaveName("CodexSwitchUsageWidget")
        usagePanel = panel
        panel.orderFrontRegardless()
        if usageDockMode == .edge, usageIsDocked { applyDockPosition(hidden: true, animated: false) }
    }

    func hideUsageWidget() {
        hideUsageTooltip()
        usagePanel?.orderOut(nil)
    }

    func setUsageDockMode(_ dockMode: UsageDockMode) {
        guard usageDockMode != dockMode else { return }
        usageDockMode = dockMode
        UserDefaults.standard.set(dockMode.rawValue, forKey: usageDockDefaultsKey)
        usageHideTask?.cancel()
        setUsageDocked(false)
        if dockMode == .edge { dockUsagePanelIfNearEdge() }
    }

    func usageWidgetHoverChanged(_ hovering: Bool) {
        guard usageDockMode == .edge, usageIsDocked else { return }
        usageHideTask?.cancel()
        if hovering {
            guard Date() >= usageIgnoreHoverUntil else { return }
            applyDockPosition(hidden: false, animated: true)
        } else {
            usageHideTask = Task { [weak self] in
                try? await Task.sleep(nanoseconds: 650_000_000)
                guard !Task.isCancelled, NSEvent.pressedMouseButtons == 0 else { return }
                self?.applyDockPosition(hidden: true, animated: true)
            }
        }
    }

    func showUsageTooltip(_ text: String?) {
        guard let text, !text.isEmpty, let usagePanel else {
            hideUsageTooltip()
            return
        }
        let tooltip = usageTooltipPanel ?? makeUsageTooltipPanel()
        tooltip.contentView = NSHostingView(rootView: UsageTooltipView(text: text))
        let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 11, weight: .medium)]
        let textWidth = ceil((text as NSString).size(withAttributes: attributes).width)
        let tooltipWidth = min(max(textWidth + 22, 120), 260)
        tooltip.setContentSize(NSSize(width: tooltipWidth, height: 28))

        guard let visibleFrame = (usagePanel.screen ?? NSScreen.main)?.visibleFrame else { return }
        let x = min(max(usagePanel.frame.midX - tooltipWidth / 2, visibleFrame.minX + 6), visibleFrame.maxX - tooltipWidth - 6)
        let y: CGFloat
        if usagePanel.frame.maxY + 34 <= visibleFrame.maxY {
            y = usagePanel.frame.maxY + 5
        } else {
            y = usagePanel.frame.minY - 33
        }
        tooltip.setFrameOrigin(NSPoint(x: x, y: y))
        tooltip.orderFrontRegardless()
    }

    func hideUsageTooltip() {
        usageTooltipPanel?.orderOut(nil)
    }

    func updateUsageWidgetDrag() {
        guard let panel = usagePanel else { return }
        hideUsageTooltip()
        usageHideTask?.cancel()
        if usageDragOrigin == nil {
            usageDragOrigin = panel.frame.origin
            usageDragMouseOrigin = NSEvent.mouseLocation
            if usageIsDocked { setUsageDocked(false) }
        }
        guard let origin = usageDragOrigin, let mouseOrigin = usageDragMouseOrigin else { return }
        let mouse = NSEvent.mouseLocation
        panel.setFrameOrigin(NSPoint(x: origin.x + mouse.x - mouseOrigin.x, y: origin.y + mouse.y - mouseOrigin.y))
    }

    func finishUsageWidgetDrag() {
        usageDragOrigin = nil
        usageDragMouseOrigin = nil
        if usageDockMode == .edge { dockUsagePanelIfNearEdge() }
    }

    private func dockUsagePanelIfNearEdge() {
        guard usageDockMode == .edge, !usagePanelIsAnimating, let panel = usagePanel,
              let visibleFrame = (panel.screen ?? NSScreen.main)?.visibleFrame else { return }
        guard let nearest = UsageDockGeometry.nearestEdge(panelFrame: panel.frame, visibleFrame: visibleFrame) else {
            setUsageDocked(false)
            return
        }
        usageDockEdge = nearest
        UserDefaults.standard.set(usageDockEdge.rawValue, forKey: usageDockEdgeDefaultsKey)
        setUsageDocked(true)
        usageIgnoreHoverUntil = Date().addingTimeInterval(0.8)
        applyDockPosition(hidden: true, animated: true)
    }

    private func setUsageDocked(_ docked: Bool) {
        usageIsDocked = docked
        UserDefaults.standard.set(docked, forKey: usageIsDockedDefaultsKey)
    }

    private func applyDockPosition(hidden: Bool, animated: Bool) {
        guard let panel = usagePanel, usageDockMode == .edge, usageIsDocked else { return }
        if hidden { hideUsageTooltip() }
        let screen = panel.screen ?? NSScreen.main
        guard let visibleFrame = screen?.visibleFrame else { return }
        let visibleSliver: CGFloat = 10
        let clampedX = min(max(panel.frame.minX, visibleFrame.minX + 8), visibleFrame.maxX - panel.frame.width - 8)
        let clampedY = min(max(panel.frame.minY, visibleFrame.minY + 8), visibleFrame.maxY - panel.frame.height - 8)
        let origin: NSPoint
        switch usageDockEdge {
        case .left:
            origin = NSPoint(x: hidden ? visibleFrame.minX - panel.frame.width + visibleSliver : visibleFrame.minX + 6, y: clampedY)
        case .right:
            origin = NSPoint(x: hidden ? visibleFrame.maxX - visibleSliver : visibleFrame.maxX - panel.frame.width - 6, y: clampedY)
        case .top:
            origin = NSPoint(x: clampedX, y: hidden ? visibleFrame.maxY - visibleSliver : visibleFrame.maxY - panel.frame.height - 6)
        case .bottom:
            origin = NSPoint(x: clampedX, y: hidden ? visibleFrame.minY - panel.frame.height + visibleSliver : visibleFrame.minY + 6)
        }
        if animated { animatePanel(panel, to: origin) } else { panel.setFrameOrigin(origin) }
    }

    private func animatePanel(_ panel: NSPanel, to origin: NSPoint) {
        usagePanelIsAnimating = true
        if NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            panel.setFrameOrigin(origin)
            usagePanelIsAnimating = false
            return
        }
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.18
            panel.animator().setFrameOrigin(origin)
        } completionHandler: { [weak self] in
            Task { @MainActor in self?.usagePanelIsAnimating = false }
        }
    }

    private func makeUsageTooltipPanel() -> NSPanel {
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 180, height: 28),
            styleMask: [.borderless, .nonactivatingPanel, .utilityWindow],
            backing: .buffered,
            defer: false
        )
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        panel.isReleasedWhenClosed = false
        panel.level = .floating
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
        usageTooltipPanel = panel
        return panel
    }

    func refreshUsage() {
        guard !isUsageLoading else { return }
        let requestedMode = mode
        let selectedChannel = self.selectedChannel
        let secret = selectedChannel.flatMap { Keychain.read(service: keychainService, account: keychainAccount(for: $0)) }
        isUsageLoading = true
        usageError = nil
        usageRefreshTask?.cancel()
        usageRefreshTask = Task { [weak self] in
            guard let self else { return }
            do {
                let snapshot: UsageSnapshot
                switch requestedMode {
                case .official:
                    let quota = try await Task.detached(priority: .utility) {
                        try UsageService.fetchOfficial()
                    }.value
                    snapshot = .official(quota)
                case .aiLink, .channel:
                    guard let selectedChannel, let secret, !secret.isEmpty else {
                        throw SwitchError.missingAPIKey(selectedChannel?.name ?? "当前渠道")
                    }
                    snapshot = .aiLink(try await UsageService.fetchChannel(channel: selectedChannel, apiKey: secret))
                case .unknown:
                    throw UsageParsingError.invalidResponse("当前渠道无法查询额度。")
                }
                guard self.mode == requestedMode else {
                    self.isUsageLoading = false
                    return
                }
                self.usageSnapshot = snapshot
                self.usageLastUpdated = Date()
                self.usageError = nil
            } catch is CancellationError {
                // A newer refresh replaced this one.
            } catch {
                guard self.mode == requestedMode else {
                    self.isUsageLoading = false
                    return
                }
                self.usageError = error.localizedDescription
            }
            self.isUsageLoading = false
        }
    }

    func openAiLinkSettings() {
        editedChannel = selectedChannel ?? .aiLink
        editedSettings = AiLinkSettings(baseURL: editedChannel.baseURL, model: editedChannel.model)
        editedAPIKey = ""
        editingChannelID = editedChannel.id
        isEditing = true
    }

    func addChannel() {
        let id = "channel-\(UUID().uuidString.prefix(8).lowercased())"
        editedChannel = ChannelProfile(id: id, name: "新建渠道", baseURL: "https://", model: "gpt-5.5", modelsPath: "/v1/models", usagePath: "/v1/usage", wireAPI: "responses", validatesModelList: true, isBuiltIn: false)
        editedSettings = AiLinkSettings(baseURL: "https://", model: "gpt-5.5")
        editedAPIKey = ""
        editingChannelID = nil
        isEditing = true
    }

    func editChannel(_ channel: ChannelProfile) {
        editedChannel = channel
        editedSettings = AiLinkSettings(baseURL: channel.baseURL, model: channel.model)
        editedAPIKey = ""
        editingChannelID = channel.id
        isEditing = true
    }

    func selectChannel(_ channel: ChannelProfile) {
        selectedChannelID = channel.id
        settings = AiLinkSettings(baseURL: channel.baseURL, model: channel.model)
        hasAPIKey = selectedChannelHasAPIKey
        saveChannelPreferences()
    }

    func deleteChannel(_ channel: ChannelProfile) {
        guard !channel.isBuiltIn else { return }
        channels.removeAll { $0.id == channel.id }
        channelModels[channel.id] = nil
        Keychain.delete(service: keychainService, account: keychainAccount(for: channel))
        saveChannelPreferences()
        if selectedChannelID == channel.id {
            selectedChannelID = ChannelProfile.aiLink.id
            if case .channel = mode { mode = .official }
        }
    }

    func persistModelPreferences() {
        if let index = channels.firstIndex(where: { $0.id == selectedChannelID }) {
            channels[index].model = settings.model
        }
        if let channel = selectedChannel, channel.id == ChannelProfile.aiLink.id {
            settings = AiLinkSettings(baseURL: channel.baseURL, model: settings.model)
        }
        saveChannelPreferences()
    }

    func saveAiLinkSettings() {
        editedChannel = selectedChannel ?? .aiLink
        saveChannelSettings()
    }

    func saveChannelSettings() {
        let name = editedChannel.normalizedName
        let baseURL = editedSettings.normalizedBaseURL
        let model = editedSettings.model.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { message = .error(SwitchError.invalidChannelName.localizedDescription); return }
        guard let url = URL(string: baseURL), url.scheme?.lowercased() == "https", url.host != nil else {
            message = .error(SwitchError.invalidBaseURL(name).localizedDescription); return
        }
        guard !model.isEmpty else { message = .error(SwitchError.invalidModel(name).localizedDescription); return }
        guard ["responses", "chat"].contains(editedChannel.wireAPI) else { message = .error(SwitchError.invalidChannelProtocol.localizedDescription); return }
        var saved = editedChannel
        saved.name = name; saved.baseURL = baseURL; saved.model = model
        if let editingChannelID, let index = channels.firstIndex(where: { $0.id == editingChannelID }) {
            channels[index] = saved
        } else {
            channels.append(saved)
        }
        if !editedAPIKey.isEmpty {
            try? Keychain.write(editedAPIKey, service: keychainService, account: keychainAccount(for: saved))
        }
        if saved.id == ChannelProfile.aiLink.id {
            settings = AiLinkSettings(baseURL: saved.baseURL, model: saved.model)
            try? fm.createDirectory(at: supportURL, withIntermediateDirectories: true)
            try? JSONEncoder.pretty.encode(settings).write(to: settingsURL, options: .atomic)
        }
        saveChannelPreferences()
        hasAPIKey = selectedChannelHasAPIKey
        isEditing = false
        message = .success("\(saved.name) 设置已保存。")
        if selectedChannelID == saved.id { refreshUsage() }
    }

    func switchTo(_ target: SwitchTarget) {
        guard !isWorking else { return }
        let selectedSettings = settings
        let selectedOfficialModel = officialModel
        let selectedChannels = channels
        let channel = target.profile
        let secret = channel.flatMap { Keychain.read(service: keychainService, account: keychainAccount(for: $0)) }
        isWorking = true
        message = .success("正在关闭 ChatGPT，随后会切换配置并覆盖全部旧任务；完成后会询问是否重启…")
        report = nil
        terminateChatGPT()
        Task {
            do {
                try await Task.sleep(nanoseconds: 1_500_000_000)
                let result = try await Task.detached(priority: .userInitiated) {
                    try await SwitchEngine.perform(target: target, settings: selectedSettings, officialModel: selectedOfficialModel, apiKey: secret, channels: selectedChannels)
                }.value
                report = result
                refresh()
                imageGenerationSkill = .forTarget(target)
                saveImageGenerationRouting(imageGenerationSkill)
                let destination = target.isOfficial ? "OpenAI 官方" : (channel?.name ?? "第三方渠道")
                message = .success("已切换到 \(destination)，全部旧任务已覆盖为当前渠道和模型。请选择立即重启或稍后手动重启 ChatGPT。")
                showRestartConfirmation = true
            } catch {
                refresh()
                message = .error(error.localizedDescription)
                openChatGPT()
            }
            isWorking = false
        }
    }

    func loginAndSwitchToOfficial() {
        guard !isWorking else { return }
        isWorking = true
        message = .success("已启动 OpenAI 登录，请在浏览器中完成授权。")
        report = nil
        let aiLinkSecret = Keychain.read(service: keychainService, account: keychainAccount)
        let selectedOfficialModel = officialModel
        let selectedChannels = channels
        let selectedSettings = settings
        Task {
            var terminatedChatGPT = false
            do {
                try await Task.detached(priority: .userInitiated) {
                    let login = try Command.runCodex(["login"], apiKey: nil)
                    guard login.status == 0 else {
                        throw SwitchError.commandFailed("OpenAI 登录未完成。\(Command.safeSummary(login.output))")
                    }
                    let status = try Command.runCodex(["login", "status"], apiKey: nil)
                    guard status.status == 0, status.output.localizedCaseInsensitiveContains("ChatGPT") else {
                        throw SwitchError.commandFailed("登录结果不是 ChatGPT 官方账号，请重试。")
                    }
                }.value
                terminateChatGPT()
                terminatedChatGPT = true
                try await Task.sleep(nanoseconds: 1_500_000_000)
                let result = try await Task.detached(priority: .userInitiated) {
                    try await SwitchEngine.perform(target: .official, settings: selectedSettings, officialModel: selectedOfficialModel, apiKey: aiLinkSecret, channels: selectedChannels)
                }.value
                report = result
                refresh()
                imageGenerationSkill = .imagegen
                saveImageGenerationRouting(.imagegen)
                message = .success("OpenAI 官方账号登录与切换已完成，全部旧任务已覆盖为官方渠道和当前模型。请选择立即重启或稍后手动重启 ChatGPT。")
                showRestartConfirmation = true
            } catch {
                refresh()
                message = .error(error.localizedDescription)
                if terminatedChatGPT { openChatGPT() }
            }
            isWorking = false
        }
    }

    func openBackupFolder() { NSWorkspace.shared.open(supportURL.appendingPathComponent("Backups")) }

    func restartChatGPT() {
        showRestartConfirmation = false
        terminateChatGPT()
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { self.openChatGPT() }
    }

    func deferRestart() {
        showRestartConfirmation = false
        message = .warning("配置已切换。ChatGPT 尚未重启，请稍后点击“重启 ChatGPT”或手动重新打开。")
    }

    private func terminateChatGPT() {
        let workspace = NSWorkspace.shared
        workspace.runningApplications
            .filter { $0.bundleIdentifier == "com.openai.chat" || $0.localizedName == "ChatGPT" }
            .forEach { $0.terminate() }
    }

    private func openChatGPT() {
        let url = URL(fileURLWithPath: "/Applications/ChatGPT.app")
        NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration()) { _, error in
            Task { @MainActor in
                if let error { self.message = .error("打开 ChatGPT 失败：\(error.localizedDescription)") }
            }
        }
    }

    private func loadSettings() {
        guard let data = try? Data(contentsOf: settingsURL),
              let saved = try? JSONDecoder().decode(AiLinkSettings.self, from: data) else { return }
        settings = saved
    }

    private func migrateAndLoadChannels() {
        if let data = try? Data(contentsOf: preferencesURL),
           let saved = try? JSONDecoder().decode(ChannelPreferences.self, from: data),
           !saved.channels.isEmpty {
            channels = saved.channels
            selectedChannelID = saved.lastChannelID ?? channels.first?.id ?? ChannelProfile.aiLink.id
            if officialModels.contains(saved.officialModel) { officialModel = saved.officialModel }
            return
        }
        var aiLink = ChannelProfile.aiLink
        aiLink.baseURL = settings.baseURL
        aiLink.model = settings.model
        channels = [aiLink]
        selectedChannelID = aiLink.id
        saveChannelPreferences()
    }

    private func saveChannelPreferences() {
        do {
            try fm.createDirectory(at: supportURL, withIntermediateDirectories: true)
            let preferences = ChannelPreferences(channels: channels, officialModel: officialModel, lastChannelID: selectedChannelID)
            try JSONEncoder.pretty.encode(preferences).write(to: preferencesURL, options: .atomic)
        } catch {
            message = .warning("渠道已应用，但偏好保存失败：\(error.localizedDescription)")
        }
    }

    private func loadPreferences() {
        guard let data = try? Data(contentsOf: preferencesURL),
              let saved = try? JSONDecoder().decode(CodexSwitchPreferences.self, from: data) else { return }
        settings = saved.aiLink
        if officialModels.contains(saved.officialModel) { officialModel = saved.officialModel }
    }

    private func savePreferences() {
        saveChannelPreferences()
    }

    private func loadImageGenerationSkill() {
        guard let data = try? Data(contentsOf: imageSkillURL),
              let saved = try? JSONDecoder().decode(ImageGenerationSkill.self, from: data) else { return }
        imageGenerationSkill = saved
    }

    private func saveImageGenerationRouting(_ skill: ImageGenerationSkill) {
        do {
            try fm.createDirectory(at: supportURL, withIntermediateDirectories: true)
            try JSONEncoder.pretty.encode(skill).write(to: imageSkillURL, options: .atomic)
        } catch {
            message = .warning("渠道已切换，但生图路由状态保存失败：\(error.localizedDescription)")
        }
    }

    private func importCurrentAiLinkConfiguration() {
        guard let text = try? String(contentsOf: configURL, encoding: .utf8),
              TOMLEditor.sectionValue("base_url", section: "model_providers.custom", in: text) != nil else { return }
        if let baseURL = TOMLEditor.sectionValue("base_url", section: "model_providers.custom", in: text),
           URL(string: baseURL)?.host != nil {
            settings.baseURL = baseURL
            if let index = channels.firstIndex(where: { $0.id == ChannelProfile.aiLink.id }) { channels[index].baseURL = baseURL }
        }
        if TOMLEditor.topLevelValue("model_provider", in: text) == SwitchEngine.providerID,
           let model = TOMLEditor.topLevelValue("model", in: text), !model.isEmpty {
            settings.model = model
            if let index = channels.firstIndex(where: { $0.id == ChannelProfile.aiLink.id }) { channels[index].model = model }
        }
        if Keychain.read(service: keychainService, account: keychainAccount) == nil {
            if let token = TOMLEditor.sectionValue("experimental_bearer_token", section: "model_providers.custom", in: text), !token.isEmpty {
                try? Keychain.write(token, service: keychainService, account: keychainAccount)
            } else if let envKey = TOMLEditor.sectionValue("env_key", section: "model_providers.custom", in: text),
                      let value = try? Launchctl.get(envKey), !value.isEmpty {
                try? Keychain.write(value, service: keychainService, account: keychainAccount)
            }
        }
        try? fm.createDirectory(at: supportURL, withIntermediateDirectories: true)
        try? JSONEncoder.pretty.encode(settings).write(to: settingsURL, options: .atomic)
        saveChannelPreferences()
    }
}

struct ContentView: View {
    @EnvironmentObject private var store: ConfigStore
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        ZStack {
            Color(nsColor: .windowBackgroundColor).ignoresSafeArea()
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    header
                    if let message = store.message { Banner(message: message) }
                    modeChoices
                    imageRouting
                    diagnostics
                    footer
                }
                .frame(maxWidth: 760, alignment: .leading)
                .padding(32)
            }
        }
        .sheet(isPresented: $store.isEditing) { ChannelEditor() }
        .confirmationDialog("配置已切换，重启 ChatGPT？", isPresented: $store.showRestartConfirmation) {
            Button("立即重启") { store.restartChatGPT() }
            Button("稍后手动重启") { store.deferRestart() }
            Button("取消", role: .cancel) { store.deferRestart() }
        } message: {
            Text("重启后，全部本地任务都会使用当前渠道和当前模型。选择稍后重启不会撤销本次配置切换。")
        }
        .onAppear { store.activateUsageWidget() }
        .onChange(of: scenePhase) { _, phase in if phase == .active { store.refresh() } }
        .animation(reduceMotion ? nil : .easeOut(duration: 0.2), value: store.message)
    }

    private var header: some View {
        HStack(alignment: .center, spacing: 16) {
            Image(systemName: "arrow.triangle.2.circlepath.circle.fill")
                .font(.system(size: 32)).foregroundStyle(Color.accentColor).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                Text("Codex Switch").font(.title2.bold())
                Text("当前：\(store.mode.title)").font(.subheadline).foregroundStyle(.secondary)
            }
            Spacer()
            Button { store.showUsageWidget() } label: {
                Label("额度悬浮窗", systemImage: "gauge.with.dots.needle.67percent")
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
            StatusBadge(mode: store.mode, conformant: store.configurationIsConformant)
        }
    }

    private var modeChoices: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("选择连接渠道").font(.headline)
            VStack(spacing: 10) {
                ModeChoice(title: "OpenAI 官方", detail: "使用 ChatGPT 登录 · \(store.officialModel)", icon: "person.crop.circle.badge.checkmark", selected: store.mode == .official, working: store.isWorking) {
                    store.switchTo(.official)
                }
                ForEach(store.channels) { channel in
                    ChannelChoice(channel: channel, selected: store.selectedChannelID == channel.id && !store.modeIsOfficial, hasKey: store.keychainValueExists(for: channel), working: store.isWorking, edit: { store.editChannel(channel) }, remove: { store.deleteChannel(channel) }) {
                        store.selectChannel(channel)
                        store.switchTo(channel.id == ChannelProfile.aiLink.id ? .aiLink : .channel(channel))
                    }
                }
            }
            HStack {
                Text("第三方渠道的密钥只存入 macOS 钥匙串，不写入配置文件。")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button { store.addChannel() } label: { Label("添加自定义渠道", systemImage: "plus.circle") }
                    .buttonStyle(.bordered).disabled(store.isWorking)
            }
            HStack(spacing: 12) {
                Picker("OpenAI 模型", selection: $store.officialModel) {
                    ForEach(store.officialModels, id: \.self) { Text($0).tag($0) }
                }
                .pickerStyle(.menu)
                .onChange(of: store.officialModel) { _, _ in store.persistModelPreferences() }

                Picker("当前第三方模型", selection: $store.settings.model) {
                    ForEach(store.selectedChannelModels, id: \.self) { Text($0).tag($0) }
                }
                .pickerStyle(.menu)
                .onChange(of: store.settings.model) { _, _ in store.persistModelPreferences() }
            }
            .controlSize(.small)
        }
    }

    private var diagnostics: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("切换检查").font(.headline)
                Spacer()
                if store.isWorking {
                    ProgressView().controlSize(.small)
                    Text("正在备份、切换并验证…").font(.caption).foregroundStyle(.secondary)
                }
            }
            VStack(spacing: 0) {
                if let report = store.report {
                    ForEach(Array(report.checks.enumerated()), id: \.element.id) { index, check in
                        CheckRow(check: check)
                        if index < report.checks.count - 1 { Divider().padding(.leading, 42) }
                    }
                } else {
                    EmptyChecks(isWorking: store.isWorking)
                }
            }
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(.separator.opacity(0.55)))
        }
    }

    private var imageRouting: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("生图路由").font(.headline)
            HStack(spacing: 12) {
                Image(systemName: "photo.on.rectangle.angled")
                    .font(.system(size: 21))
                    .foregroundStyle(Color.accentColor)
                    .frame(width: 38, height: 38)
                    .background(Color.accentColor.opacity(0.11), in: RoundedRectangle(cornerRadius: 7))
                VStack(alignment: .leading, spacing: 3) {
                    Text("当前默认：\(store.imageGenerationSkill.displayName)").font(.callout.weight(.medium))
                    Text(store.imageGenerationSkill.detail).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.green)
            }
            .padding(14)
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(.separator.opacity(0.55)))
            Text("官方渠道使用 $imagegen；AiLink 使用 $imagegen2。技能名称只影响本应用记录的默认路由，已打开的旧任务不会改变。")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 14) {
            Label("切换会批量覆盖全部旧任务；完成后由你确认立即重启或稍后手动重启 ChatGPT。正在运行的任务会中断。", systemImage: "exclamationmark.arrow.triangle.2.circlepath")
                .font(.callout).foregroundStyle(.secondary)
            HStack {
                Button { store.loginAndSwitchToOfficial() } label: { Label("登录并切换 OpenAI", systemImage: "person.badge.key") }
                    .buttonStyle(.bordered).disabled(store.isWorking)
                Button { store.openBackupFolder() } label: { Label("查看备份", systemImage: "folder") }.buttonStyle(.bordered)
                Spacer()
                Button { store.showRestartConfirmation = true } label: { Label("重启 ChatGPT", systemImage: "arrow.clockwise") }
                    .buttonStyle(.borderedProminent).disabled(store.isWorking)
            }
            .frame(maxWidth: .infinity)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct UsageWidgetView: View {
    @EnvironmentObject private var store: ConfigStore
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 5) {
            Circle()
                .fill(statusColor)
                .frame(width: 6, height: 6)
                .overlay(Circle().stroke(.background, lineWidth: 1))
                .accessibilityLabel(store.mode.title)

            if let snapshot = store.usageSnapshot, snapshotMatchesMode(snapshot) {
                snapshotContent(snapshot)
                    .layoutPriority(1)
            } else if store.isUsageLoading {
                ProgressView().controlSize(.mini).frame(maxWidth: .infinity)
            } else {
                Text("读取失败")
                    .font(.system(size: 9.5, weight: .medium))
                    .foregroundStyle(Color.orange)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .help(store.usageError ?? "暂时无法读取额度")
            }

            Button { store.refreshUsage() } label: {
                Image(systemName: "arrow.clockwise")
                    .rotationEffect(.degrees(store.isUsageLoading ? 360 : 0))
                    .animation(reduceMotion ? nil : store.isUsageLoading ? .linear(duration: 0.9).repeatForever(autoreverses: false) : .default, value: store.isUsageLoading)
                    .font(.system(size: 9.5, weight: .medium))
                    .frame(width: 14, height: 18)
            }
            .buttonStyle(.borderless)
            .disabled(store.isUsageLoading)
            .help(refreshHelp)
            .accessibilityLabel("刷新额度")

            Menu {
                ForEach(UsageDockMode.allCases, id: \.rawValue) { mode in
                    Button {
                        store.setUsageDockMode(mode)
                    } label: {
                        if store.usageDockMode == mode {
                            Label(mode.title, systemImage: "checkmark")
                        } else {
                            Text(mode.title)
                        }
                    }
                }
                Divider()
                Button("关闭悬浮") { store.hideUsageWidget() }
            } label: {
                Image(systemName: "ellipsis")
                    .font(.system(size: 10, weight: .semibold))
                    .frame(width: 14, height: 18)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .accessibilityLabel("悬浮设置")
        }
        .padding(.horizontal, 6)
        .frame(width: 160, height: 34)
        .background(.ultraThinMaterial, in: Capsule())
        .overlay(Capsule().stroke(.white.opacity(0.32), lineWidth: 0.8))
        .contentShape(Capsule())
        .onHover { store.usageWidgetHoverChanged($0) }
        .simultaneousGesture(
            DragGesture(minimumDistance: 3)
                .onChanged { _ in store.updateUsageWidgetDrag() }
                .onEnded { _ in store.finishUsageWidgetDrag() }
        )
    }

    @ViewBuilder
    private func snapshotContent(_ snapshot: UsageSnapshot) -> some View {
        switch snapshot {
        case .official(let quota):
            HStack(spacing: 4) {
                MicroQuota(title: "5h", window: quota.fiveHour) { store.showUsageTooltip($0) }
                Divider().frame(height: 17)
                MicroQuota(title: "周", window: quota.weekly) { store.showUsageTooltip($0) }
            }
            .frame(maxWidth: .infinity)
            .fixedSize(horizontal: true, vertical: false)
        case .aiLink(let balance):
            HStack(spacing: 3) {
                Text("AiLink").font(.system(size: 8.5)).foregroundStyle(.secondary)
                Text(balanceText(balance.remaining))
                    .font(.system(size: 11, weight: .bold, design: .rounded))
                    .foregroundStyle(balance.remaining > 0 ? Color.primary : Color.red)
                    .contentTransition(.numericText())
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func snapshotMatchesMode(_ snapshot: UsageSnapshot) -> Bool {
        switch (store.mode, snapshot) {
        case (.official, .official), (.aiLink, .aiLink), (.channel, .aiLink): true
        default: false
        }
    }

    private var statusColor: Color {
        if store.usageError != nil { return .orange }
        if store.isUsageLoading { return .blue }
        return .green
    }

    private var refreshHelp: String {
        if let error = store.usageError { return "实时更新失败：\(error)" }
        guard let date = store.usageLastUpdated else { return "立即刷新；每 10 秒实时更新" }
        return "每 10 秒实时更新 · 上次 \(date.formatted(date: .omitted, time: .standard))"
    }

    private func balanceText(_ balance: Double) -> String {
        let formatter = NumberFormatter()
        formatter.numberStyle = .currency
        formatter.currencyCode = "USD"
        formatter.currencySymbol = "$"
        formatter.minimumFractionDigits = 2
        formatter.maximumFractionDigits = balance < 1 ? 4 : 2
        return formatter.string(from: NSNumber(value: balance)) ?? String(format: "$%.2f", balance)
    }
}

struct MicroQuota: View {
    let title: String
    let window: QuotaWindowSnapshot?
    let tooltipChanged: (String?) -> Void

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 2) {
            Text(title).font(.system(size: 8, weight: .medium)).foregroundStyle(.secondary).lineLimit(1)
            Text(window.map { "\(Int($0.remainingPercent.rounded()))%" } ?? "—")
                .font(.system(size: 10.5, weight: .bold, design: .rounded))
                .foregroundStyle(window.map { color(for: $0.remainingPercent) } ?? .secondary)
                .contentTransition(.numericText())
                .lineLimit(1)
        }
        .frame(minWidth: 35, alignment: .leading)
        .fixedSize(horizontal: true, vertical: true)
        .onHover { tooltipChanged($0 ? helpText : nil) }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(title)额度")
        .accessibilityValue(window.map { "剩余 \(Int($0.remainingPercent.rounded()))%" } ?? "不可用")
    }

    private var helpText: String {
        guard let window else { return "官方暂未提供此额度窗口" }
        guard let reset = window.resetsAt else { return "剩余 \(Int(window.remainingPercent.rounded()))%" }
        let resetText = reset.formatted(.dateTime.month().day().hour().minute())
        let label = title == "5h" ? "5 小时额度" : "周额度"
        return "\(label)：剩余 \(Int(window.remainingPercent.rounded()))%，\(resetText) 重置"
    }

    private func color(for remaining: Double) -> Color {
        if remaining <= 10 { return .red }
        if remaining <= 30 { return .orange }
        return .green
    }
}

struct ModeChoice: View {
    let title: String, detail: String, icon: String
    let selected: Bool, working: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 14) {
                Image(systemName: icon).font(.system(size: 23)).frame(width: 42, height: 42)
                    .background((selected ? Color.accentColor : Color.secondary).opacity(0.12), in: RoundedRectangle(cornerRadius: 7))
                VStack(alignment: .leading, spacing: 4) {
                    Text(title).font(.headline)
                    Text(detail).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 8)
                Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(selected ? Color.accentColor : Color.secondary).accessibilityHidden(true)
            }
            .padding(16).frame(maxWidth: .infinity, minHeight: 84).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(selected ? Color.accentColor : Color(nsColor: .separatorColor), lineWidth: selected ? 2 : 1))
        .disabled(working)
        .accessibilityLabel("切换到\(title)").accessibilityValue(selected ? "当前已选择" : "未选择")
    }
}

struct StatusBadge: View {
    let mode: ProviderMode
    let conformant: Bool
    var body: some View {
        let isUnknown: Bool = if case .unknown = mode { true } else { false }
        let good = conformant && !isUnknown
        Label(good ? "配置正常" : "需要修复", systemImage: good ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
            .font(.caption.weight(.semibold)).foregroundStyle(good ? Color.green : Color.orange)
            .padding(.horizontal, 11).padding(.vertical, 7)
            .background((good ? Color.green : Color.orange).opacity(0.1), in: Capsule())
    }
}

struct Banner: View {
    let message: BannerMessage
    var body: some View {
        let color: Color = switch message.kind {
        case .success: .green
        case .error: .red
        case .warning: .orange
        }
        let icon = switch message.kind {
        case .success: "checkmark.circle.fill"
        case .error: "xmark.octagon.fill"
        case .warning: "exclamationmark.triangle.fill"
        }
        Label(message.text, systemImage: icon)
            .font(.callout).foregroundStyle(color).frame(maxWidth: .infinity, alignment: .leading).padding(14)
            .background(color.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
    }
}

struct EmptyChecks: View {
    let isWorking: Bool
    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: isWorking ? "hourglass" : "checklist").foregroundStyle(.secondary).frame(width: 24)
            Text(isWorking ? "正在执行完整检查" : "完成一次切换后，这里会显示检查结果。")
                .font(.callout).foregroundStyle(.secondary)
            Spacer()
        }.padding(16)
    }
}

struct CheckRow: View {
    let check: CheckResult
    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: check.state == .passed ? "checkmark.circle.fill" : check.state == .warning ? "exclamationmark.triangle.fill" : "xmark.circle.fill")
                .foregroundStyle(check.state == .passed ? Color.green : check.state == .warning ? Color.orange : Color.red).frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(check.title).font(.callout.weight(.medium))
                Text(check.detail).font(.caption).foregroundStyle(.secondary).lineLimit(2)
            }
            Spacer()
        }.padding(14)
    }
}

struct ChannelChoice: View {
    let channel: ChannelProfile
    let selected: Bool
    let hasKey: Bool
    let working: Bool
    let edit: () -> Void
    let remove: () -> Void
    let action: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Button(action: action) {
                HStack(spacing: 14) {
                    Image(systemName: channel.isBuiltIn ? "network" : "server.rack")
                        .font(.system(size: 23)).frame(width: 42, height: 42)
                        .background((selected ? Color.accentColor : Color.secondary).opacity(0.12), in: RoundedRectangle(cornerRadius: 7))
                    VStack(alignment: .leading, spacing: 4) {
                        Text(channel.name).font(.headline)
                        Text("\(channel.model) · \(hasKey ? "密钥已保存" : "未配置密钥")").font(.caption).foregroundStyle(hasKey ? .secondary : Color.orange).lineLimit(1)
                    }
                    Spacer(minLength: 8)
                    Image(systemName: selected ? "checkmark.circle.fill" : "circle").foregroundStyle(selected ? Color.accentColor : Color.secondary).accessibilityHidden(true)
                }
                .padding(16).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(working)
            Button(action: edit) { Image(systemName: "gearshape") }.buttonStyle(.borderless).help("编辑渠道").accessibilityLabel("编辑\(channel.name)")
            if !channel.isBuiltIn {
                Button(role: .destructive, action: remove) { Image(systemName: "trash") }.buttonStyle(.borderless).help("删除渠道").accessibilityLabel("删除\(channel.name)")
            }
        }
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(selected ? Color.accentColor : Color(nsColor: .separatorColor), lineWidth: selected ? 2 : 1))
    }
}

struct ChannelEditor: View {
    @EnvironmentObject private var store: ConfigStore
    @State private var showsAPIKey = false

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("\(store.editedChannel.name) 设置").font(.title2.bold())
                Spacer()
                Button("取消") { store.isEditing = false }.keyboardShortcut(.cancelAction)
            }.padding(22)
            Divider()
            Form {
                Section("连接") {
                    TextField("渠道名称", text: $store.editedChannel.name)
                    TextField("API 地址", text: $store.editedSettings.baseURL)
                    TextField("模型", text: $store.editedSettings.model)
                    TextField("模型列表路径", text: $store.editedChannel.modelsPath)
                    TextField("余额路径", text: $store.editedChannel.usagePath)
                    Picker("API 协议", selection: $store.editedChannel.wireAPI) {
                        Text("Responses").tag("responses")
                        Text("Chat Completions").tag("chat")
                    }
                    Toggle("切换前校验 /v1/models", isOn: $store.editedChannel.validatesModelList)
                }
                Section("认证") {
                    VStack(alignment: .leading, spacing: 8) {
                            Text("API Key").font(.callout.weight(.medium))
                        HStack(spacing: 8) {
                            Group {
                                if showsAPIKey {
                                    TextField("新 API Key", text: $store.editedAPIKey, prompt: Text(store.editingChannelHasAPIKey ? "输入新密钥以替换现有密钥" : "输入当前渠道 API Key"))
                                } else {
                                    SecureField("新 API Key", text: $store.editedAPIKey, prompt: Text(store.editingChannelHasAPIKey ? "输入新密钥以替换现有密钥" : "输入当前渠道 API Key"))
                                }
                            }
                            .labelsHidden()
                            .textFieldStyle(.roundedBorder)
                            .frame(minHeight: 28)

                            Button { showsAPIKey.toggle() } label: {
                                Image(systemName: showsAPIKey ? "eye.slash" : "eye")
                                    .frame(width: 20, height: 20)
                            }
                            .buttonStyle(.borderless)
                            .help(showsAPIKey ? "隐藏正在输入的密钥" : "显示正在输入的密钥")
                            .accessibilityLabel(showsAPIKey ? "隐藏新密钥" : "显示新密钥")
                        }
                        Text(store.editingChannelHasAPIKey ? "留空可保留钥匙串中的现有密钥。" : "请输入当前第三方渠道提供的 API Key。")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Label("密钥保存在 macOS 钥匙串中", systemImage: "lock.fill")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Section("固定兼容设置") {
                    LabeledContent("API 协议", value: store.editedChannel.wireAPI == "chat" ? "Chat Completions" : "Responses")
                    LabeledContent("WebSocket", value: "关闭")
                    LabeledContent("官方认证", value: "不使用")
                }
            }.formStyle(.grouped)
            Divider()
            HStack {
                Spacer()
                Button("保存") { store.saveChannelSettings() }.buttonStyle(.borderedProminent).controlSize(.large).keyboardShortcut(.defaultAction)
            }.padding(18)
        }.frame(width: 560, height: 640)
    }
}

enum SwitchEngine {
    static let providerID = "custom"
    static let environmentKey = "AILINK_API_KEY"
    static let imageSkillEnvironmentKey = "CODEX_SWITCH_IMAGE_SKILL"

    static func perform(target: SwitchTarget, settings: AiLinkSettings, officialModel: String, apiKey: String?, channels: [ChannelProfile] = []) async throws -> SwitchReport {
        let fm = FileManager.default
        let home = fm.homeDirectoryForCurrentUser
        let configURL = home.appendingPathComponent(".codex/config.toml")
        let supportURL = home.appendingPathComponent("Library/Application Support/CodexSwitch")
        let original = (try? String(contentsOf: configURL, encoding: .utf8)) ?? ""
        let targetChannel = target.profile.flatMap { profile in channels.first(where: { $0.id == profile.id }) } ?? target.profile
        let targetEnvironmentKey = targetChannel.map(environmentKey(for:)) ?? environmentKey
        let previousEnvironment = try? Launchctl.get(targetEnvironmentKey)
        let previousImageSkill = try? Launchctl.get(imageSkillEnvironmentKey)
        let backupURL = try backup(original, in: supportURL, fileManager: fm)

        do {
            let legacyToken = TOMLEditor.sectionValue("experimental_bearer_token", section: "model_providers.\(providerID)", in: original)
            let retainedKey = apiKey ?? legacyToken ?? previousEnvironment
            var updated = original
            switch target {
            case .official:
                guard ["gpt-5.2", "gpt-5.5", "gpt-5.6-luna", "gpt-5.6-terra", "gpt-5.6-sol"].contains(officialModel) else { throw SwitchError.invalidModel("OpenAI 官方") }
                updated = TOMLEditor.settingTopLevel("model", value: officialModel, in: updated)
                if let retainedKey, !retainedKey.isEmpty,
                   let url = URL(string: settings.normalizedBaseURL), url.scheme == "https", url.host != nil,
                   !settings.model.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    updated = TOMLEditor.removingSection("model_providers.\(providerID)", from: updated)
                    updated = appendAiLinkSection(to: updated, settings: settings)
                    try Launchctl.set(environmentKey, value: retainedKey)
                }
                updated = TOMLEditor.removingTopLevel("model_provider", from: updated)
                try writeConfig(updated, to: configURL, fileManager: fm)
            case .aiLink, .channel:
                guard let channel = targetChannel else { throw SwitchError.validationFailed("未找到目标渠道配置。") }
                let profile = channel.id == ChannelProfile.aiLink.id ? ChannelProfile(id: channel.id, name: channel.name, baseURL: settings.baseURL, model: settings.model, modelsPath: channel.modelsPath, usagePath: channel.usagePath, wireAPI: channel.wireAPI, validatesModelList: channel.validatesModelList, isBuiltIn: true) : channel
                guard let apiKey, !apiKey.isEmpty else { throw SwitchError.missingAPIKey(profile.name) }
                guard let url = URL(string: profile.normalizedBaseURL), url.scheme?.lowercased() == "https", url.host != nil else { throw SwitchError.invalidBaseURL(profile.name) }
                guard !profile.normalizedModel.isEmpty else { throw SwitchError.invalidModel(profile.name) }
                guard ["responses", "chat"].contains(profile.wireAPI) else { throw SwitchError.invalidChannelProtocol }
                if profile.validatesModelList {
                    let supportedModels = try await fetchModelIDs(channel: profile, apiKey: apiKey)
                    guard supportedModels.contains(profile.normalizedModel) else {
                        throw SwitchError.validationFailed("\(profile.name) 不支持模型 \(profile.normalizedModel)，请从模型菜单选择可用项。")
                    }
                }
                let activeProviderID = providerID(for: profile)
                updated = TOMLEditor.removingSection("model_providers.\(activeProviderID)", from: updated)
                updated = TOMLEditor.settingTopLevel("model_provider", value: activeProviderID, in: updated)
                updated = TOMLEditor.settingTopLevel("model", value: profile.normalizedModel, in: updated)
                updated = appendChannelSection(to: updated, channel: profile, envKey: targetEnvironmentKey)
                try Launchctl.set(targetEnvironmentKey, value: apiKey)
                try writeConfig(updated, to: configURL, fileManager: fm)
            }

            try Launchctl.set(imageSkillEnvironmentKey, value: ImageGenerationSkill.forTarget(target).rawValue)

            let doctor = try Command.runCodex(["doctor", "--summary", "--ascii", "--no-color"], apiKey: target.isOfficial ? nil : apiKey, environmentKey: targetChannel.map(environmentKey(for:)) ?? environmentKey)
            guard Command.doctorChecksPassed(doctor.output, target: target) else {
                throw SwitchError.validationFailed(Command.safeSummary(doctor.output))
            }
            var checks = [
                CheckResult(title: "配置文件", detail: "格式与 Provider 设置有效", state: .passed),
                CheckResult(title: "Codex Doctor", detail: "配置与认证检查通过", state: .passed)
            ]
            switch target {
            case .official:
                let login = try Command.runCodex(["login", "status"], apiKey: nil)
                guard login.status == 0 else { throw SwitchError.validationFailed("官方登录状态无效。") }
                let usesChatGPT = login.output.localizedCaseInsensitiveContains("ChatGPT")
                guard usesChatGPT else { throw SwitchError.validationFailed("当前不是 ChatGPT 账号登录，请先完成官方登录。") }
                checks.insert(CheckResult(title: "官方登录", detail: "ChatGPT 账号登录有效", state: .passed), at: 1)
                if TOMLEditor.sectionValue("base_url", section: "model_providers.\(providerID)", in: updated) != nil {
                    checks.insert(CheckResult(title: "备用 Provider", detail: "已保留 custom 定义，任务索引统一使用官方 Provider", state: .passed), at: 1)
                }
                if doctor.output.localizedCaseInsensitiveContains("WebSocket failed") {
                    checks.append(CheckResult(title: "官方 WebSocket", detail: "连接失败，Codex 将自动使用 HTTPS；若反复重连，请检查代理或防火墙", state: .warning))
                }
            case .aiLink, .channel:
                guard let channel = targetChannel, let apiKey else { throw SwitchError.missingAPIKey(targetChannel?.name ?? "当前渠道") }
                let endpoint = try await endpointStatus(baseURL: channel.normalizedBaseURL, apiKey: apiKey, channelName: channel.name)
                checks.insert(CheckResult(title: "\(channel.name) 服务", detail: endpoint, state: .passed), at: 1)
                checks.insert(CheckResult(title: "密钥注入", detail: "已通过环境变量读取，配置文件不含明文密钥", state: .passed), at: 1)
            }
            let activeProvider = target.isOfficial ? "openai" : providerID(for: targetChannel ?? .aiLink)
            let activeModel = target.isOfficial ? officialModel : (targetChannel?.normalizedModel ?? settings.model)
            if let rebound = try SessionRebinder.rebindAll(
                databaseURL: home.appendingPathComponent(".codex/state_5.sqlite"),
                backupDirectory: supportURL.appendingPathComponent("Backups"),
                provider: activeProvider,
                model: activeModel
            ) {
                checks.append(CheckResult(
                    title: "全部旧任务",
                    detail: "已覆盖 \(rebound.changedCount) 个任务为 \(activeProvider) / \(activeModel)",
                    state: .passed
                ))
                checks.append(CheckResult(title: "任务索引备份", detail: rebound.backupURL.lastPathComponent, state: .passed))
            } else {
                checks.append(CheckResult(title: "全部旧任务", detail: "未找到本地任务索引；新任务仍使用当前渠道", state: .warning))
            }
            checks.append(CheckResult(title: "配置备份", detail: backupURL.lastPathComponent, state: .passed))
            return SwitchReport(checks: checks, backupURL: backupURL)
        } catch {
            try? writeConfig(original, to: configURL, fileManager: fm)
            if let previousEnvironment, !previousEnvironment.isEmpty {
                try? Launchctl.set(targetEnvironmentKey, value: previousEnvironment)
            } else {
                try? Launchctl.unset(targetEnvironmentKey)
            }
            if let previousImageSkill, !previousImageSkill.isEmpty {
                try? Launchctl.set(imageSkillEnvironmentKey, value: previousImageSkill)
            } else {
                try? Launchctl.unset(imageSkillEnvironmentKey)
            }
            throw error
        }
    }

    static func providerID(for channel: ChannelProfile) -> String {
        channel.id == ChannelProfile.aiLink.id ? providerID : "custom_\(channel.id.replacingOccurrences(of: "-", with: "_"))"
    }

    static func environmentKey(for channel: ChannelProfile) -> String {
        channel.id == ChannelProfile.aiLink.id ? environmentKey : "CODEX_\(channel.id.uppercased().replacingOccurrences(of: "-", with: "_"))_API_KEY"
    }

    private static func appendChannelSection(to text: String, channel: ChannelProfile, envKey: String) -> String {
        let id = providerID(for: channel)
        return text.trimmingCharacters(in: .newlines) + """


[model_providers.\(id)]
name = \(TOMLEditor.quoted(channel.normalizedName))
base_url = \(TOMLEditor.quoted(channel.normalizedBaseURL))
env_key = \(TOMLEditor.quoted(envKey))
wire_api = \(TOMLEditor.quoted(channel.wireAPI))
requires_openai_auth = false
supports_websockets = false
""" + "\n"
    }

    private static func appendAiLinkSection(to text: String, settings: AiLinkSettings) -> String {
        appendChannelSection(to: text, channel: ChannelProfile(id: ChannelProfile.aiLink.id, name: "AiLink", baseURL: settings.baseURL, model: settings.model, modelsPath: "/v1/models", usagePath: "/v1/usage", wireAPI: "responses", validatesModelList: true, isBuiltIn: true), envKey: environmentKey)
    }

    static func isConformantAiLinkConfig(_ text: String) -> Bool {
        isConformantChannelConfig(text, channel: .aiLink)
    }

    static func isConformantChannelConfig(_ text: String, channel: ChannelProfile) -> Bool {
        let section = "model_providers.\(providerID(for: channel))"
        return TOMLEditor.sectionValue("base_url", section: section, in: text) == channel.normalizedBaseURL &&
        TOMLEditor.sectionValue("wire_api", section: section, in: text) == channel.wireAPI &&
        TOMLEditor.sectionValue("env_key", section: section, in: text) == environmentKey(for: channel) &&
        TOMLEditor.sectionValue("requires_openai_auth", section: section, in: text) == "false" &&
        TOMLEditor.sectionValue("supports_websockets", section: section, in: text) == "false" &&
        TOMLEditor.sectionValue("experimental_bearer_token", section: section, in: text) == nil
    }

    private static func endpointStatus(baseURL: String, apiKey: String, channelName: String = "渠道") async throws -> String {
        guard let url = URL(string: baseURL) else { throw SwitchError.invalidBaseURL("AiLink") }
        var request = URLRequest(url: url)
        request.timeoutInterval = 10
        request.httpMethod = "GET"
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        let (_, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse else { throw SwitchError.validationFailed("\(channelName) 未返回有效的 HTTP 响应。") }
        guard http.statusCode < 500 else { throw SwitchError.validationFailed("\(channelName) 服务返回 HTTP \(http.statusCode)。") }
        return "服务可达（HTTP \(http.statusCode)）"
    }

    private static func fetchModelIDs(channel: ChannelProfile, apiKey: String) async throws -> Set<String> {
        guard let url = channel.endpoint(path: channel.modelsPath) else { throw SwitchError.invalidBaseURL(channel.name) }
        var request = URLRequest(url: url)
        request.timeoutInterval = 10
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw SwitchError.validationFailed("无法读取 \(channel.name) 模型列表，请检查地址或密钥。")
        }
        struct ModelList: Decodable { struct Item: Decodable { let id: String }; let data: [Item] }
        guard let list = try? JSONDecoder().decode(ModelList.self, from: data) else {
            throw SwitchError.validationFailed("\(channel.name) 模型列表格式无法识别。")
        }
        return Set(list.data.map(\.id))
    }

    private static func backup(_ text: String, in supportURL: URL, fileManager: FileManager) throws -> URL {
        let directory = supportURL.appendingPathComponent("Backups")
        try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyyMMdd-HHmmss-SSS"
        let url = directory.appendingPathComponent("config-\(formatter.string(from: Date())).toml")
        try Data(text.utf8).write(to: url, options: .atomic)
        return url
    }

    private static func writeConfig(_ text: String, to configURL: URL, fileManager: FileManager) throws {
        try fileManager.createDirectory(at: configURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        let temporaryURL = configURL.deletingLastPathComponent().appendingPathComponent(".config.codex-switch.tmp")
        try Data(text.utf8).write(to: temporaryURL, options: .atomic)
        if fileManager.fileExists(atPath: configURL.path) {
            _ = try fileManager.replaceItemAt(configURL, withItemAt: temporaryURL)
        } else {
            try fileManager.moveItem(at: temporaryURL, to: configURL)
        }
    }
}

enum SessionRebinder {
    static func rebindAll(databaseURL: URL, backupDirectory: URL, provider: String, model: String) throws -> SessionRebindReport? {
        let fm = FileManager.default
        guard fm.fileExists(atPath: databaseURL.path) else { return nil }
        try fm.createDirectory(at: backupDirectory, withIntermediateDirectories: true)

        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyyMMdd-HHmmss-SSS"
        let backupURL = backupDirectory.appendingPathComponent("state-\(formatter.string(from: Date())).sqlite")
        let backupCommand = ".backup \(sqlDotCommandArgument(backupURL.path))"
        let backup = try runSQLite(databaseURL: databaseURL, arguments: [backupCommand])
        guard backup.status == 0, fm.fileExists(atPath: backupURL.path) else {
            throw SwitchError.commandFailed("无法备份 Codex 任务索引。\(Command.safeSummary(backup.output))")
        }

        let providerValue = sqlLiteral(provider)
        let modelValue = sqlLiteral(model)
        let sql = """
        PRAGMA busy_timeout=10000;
        BEGIN IMMEDIATE;
        UPDATE threads
        SET model_provider = \(providerValue), model = \(modelValue)
        WHERE (model_provider = 'openai' OR model_provider = 'custom' OR model_provider LIKE 'custom_%') AND preview <> '';
        SELECT changes();
        COMMIT;
        """
        let result = try runSQLite(databaseURL: databaseURL, arguments: [sql])
        guard result.status == 0 else {
            throw SwitchError.commandFailed("无法覆盖旧任务的渠道设置。\(Command.safeSummary(result.output))")
        }
        let changedCount = result.output.components(separatedBy: .newlines)
            .compactMap { Int($0.trimmingCharacters(in: .whitespacesAndNewlines)) }
            .last ?? 0

        let verifySQL = "SELECT COUNT(*) FROM threads WHERE (model_provider = 'openai' OR model_provider = 'custom' OR model_provider LIKE 'custom_%') AND preview <> '' AND (model_provider <> \(providerValue) OR model <> \(modelValue));"
        let verification = try runSQLite(databaseURL: databaseURL, arguments: [verifySQL])
        guard verification.status == 0,
              verification.output.trimmingCharacters(in: .whitespacesAndNewlines) == "0" else {
            throw SwitchError.commandFailed("旧任务渠道设置验证失败；任务索引备份已保留。")
        }
        return SessionRebindReport(changedCount: changedCount, backupURL: backupURL)
    }

    private static func runSQLite(databaseURL: URL, arguments: [String]) throws -> CommandResult {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/sqlite3")
        process.arguments = [databaseURL.path] + arguments
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        try process.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return CommandResult(status: process.terminationStatus, output: String(data: data, encoding: .utf8) ?? "")
    }

    private static func sqlLiteral(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: "''") + "'"
    }

    private static func sqlDotCommandArgument(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: "''") + "'"
    }
}

enum TOMLEditor {
    static func quoted(_ value: String) -> String {
        "\"" + value.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\""
    }

    static func topLevelValue(_ key: String, in text: String) -> String? {
        value(key, in: ArraySlice(text.components(separatedBy: .newlines).prefix { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("[") }))
    }

    static func sectionValue(_ key: String, section: String, in text: String) -> String? {
        let lines = text.components(separatedBy: .newlines)
        guard let start = lines.firstIndex(where: { $0.trimmingCharacters(in: .whitespaces) == "[\(section)]" }) else { return nil }
        let body = lines[(start + 1)...].prefix { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("[") }
        return value(key, in: body)
    }

    static func settingTopLevel(_ key: String, value: String, in text: String) -> String {
        let replacement = "\(key) = \(quoted(value))"
        var lines = text.components(separatedBy: .newlines)
        let sectionStart = lines.firstIndex(where: { $0.trimmingCharacters(in: .whitespaces).hasPrefix("[") }) ?? lines.count
        for index in 0..<sectionStart where assignmentKey(in: lines[index]) == key {
            lines[index] = replacement
            return lines.joined(separator: "\n")
        }
        lines.insert(replacement, at: sectionStart)
        return lines.joined(separator: "\n")
    }

    static func removingTopLevel(_ key: String, from text: String) -> String {
        var insideSection = false
        return text.components(separatedBy: .newlines).filter { line in
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("[") { insideSection = true }
            return insideSection || assignmentKey(in: line) != key
        }.joined(separator: "\n")
    }

    static func removingSection(_ name: String, from text: String) -> String {
        var output: [String] = [], skipping = false
        for line in text.components(separatedBy: .newlines) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("[") && trimmed.hasSuffix("]") { skipping = trimmed == "[\(name)]" }
            if !skipping { output.append(line) }
        }
        while output.last?.isEmpty == true { output.removeLast() }
        return output.joined(separator: "\n") + "\n"
    }

    private static func value(_ key: String, in lines: ArraySlice<String>) -> String? {
        guard let line = lines.first(where: { assignmentKey(in: $0) == key }), let equals = line.firstIndex(of: "=") else { return nil }
        var raw = String(line[line.index(after: equals)...]).trimmingCharacters(in: .whitespaces)
        if let comment = raw.firstIndex(of: "#") { raw = String(raw[..<comment]).trimmingCharacters(in: .whitespaces) }
        if raw.count >= 2, raw.first == "\"", raw.last == "\"" { raw.removeFirst(); raw.removeLast() }
        return raw.replacingOccurrences(of: "\\\"", with: "\"").replacingOccurrences(of: "\\\\", with: "\\")
    }

    private static func assignmentKey(in line: String) -> String? {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard !trimmed.hasPrefix("#"), !trimmed.hasPrefix("["), let equals = trimmed.firstIndex(of: "=") else { return nil }
        return String(trimmed[..<equals]).trimmingCharacters(in: .whitespaces)
    }
}

enum Keychain {
    static func write(_ value: String, service: String, account: String) throws {
        delete(service: service, account: account)
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account, kSecValueData as String: Data(value.utf8), kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlock]
        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else { throw NSError(domain: NSOSStatusErrorDomain, code: Int(status)) }
    }
    static func read(service: String, account: String) -> String? {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account, kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne]
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }
    static func delete(service: String, account: String) {
        SecItemDelete([kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account] as CFDictionary)
    }
}

enum Launchctl {
    static func set(_ key: String, value: String) throws { _ = try run(["setenv", key, value]) }
    static func unset(_ key: String) throws { _ = try run(["unsetenv", key]) }
    static func get(_ key: String) throws -> String { try run(["getenv", key]).trimmingCharacters(in: .whitespacesAndNewlines) }
    private static func run(_ arguments: [String]) throws -> String {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/launchctl")
        process.arguments = arguments
        let output = Pipe(), error = Pipe()
        process.standardOutput = output
        process.standardError = error
        try process.run()
        process.waitUntilExit()
        let stdout = output.fileHandleForReading.readDataToEndOfFile()
        let stderr = error.fileHandleForReading.readDataToEndOfFile()
        guard process.terminationStatus == 0 else {
            let detail = String(data: stderr, encoding: .utf8) ?? "launchctl 执行失败"
            throw SwitchError.commandFailed(detail.trimmingCharacters(in: .whitespacesAndNewlines))
        }
        return String(data: stdout, encoding: .utf8) ?? ""
    }
}

struct CommandResult: Sendable { let status: Int32; let output: String }

enum Command {
    static func runCodex(_ arguments: [String], apiKey: String?, environmentKey: String = SwitchEngine.environmentKey) throws -> CommandResult {
        let candidates = ["/Applications/ChatGPT.app/Contents/Resources/codex", "/opt/homebrew/bin/codex", "/usr/local/bin/codex"]
        guard let executable = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else {
            throw SwitchError.commandFailed("找不到 Codex 命令行工具。")
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        if let apiKey {
            var environment = ProcessInfo.processInfo.environment
            environment[environmentKey] = apiKey
            process.environment = environment
        }
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        try process.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return CommandResult(status: process.terminationStatus, output: String(data: data, encoding: .utf8) ?? "")
    }

    static func safeSummary(_ output: String) -> String {
        let lines = output.replacingOccurrences(of: "sk-[A-Za-z0-9_-]+", with: "<已隐藏>", options: .regularExpression)
            .components(separatedBy: .newlines).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        return String(lines.suffix(3).joined(separator: " · ").prefix(220))
    }

    static func doctorChecksPassed(_ output: String, target: SwitchTarget) -> Bool {
        let required = output.contains("[ok] config") && output.contains("[ok] auth")
        guard required else { return false }
        if !target.isOfficial {
            return output.contains("[ok] websocket") && output.localizedCaseInsensitiveContains("WebSocket is not enabled")
        }
        return true
    }
}

extension JSONEncoder {
    static var pretty: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        return encoder
    }
}
