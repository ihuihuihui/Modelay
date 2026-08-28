use crate::error::{ModelayError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn platform_label() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(target_os = "macos")]
pub fn configure_usage_window(window: &tauri::WebviewWindow) -> Result<()> {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};

    let pointer = window.ns_window()?;
    if pointer.is_null() {
        return Err("无法取得额度悬浮窗的 macOS 原生句柄。".into());
    }
    unsafe {
        let native = &*pointer.cast::<NSWindow>();
        native.setStyleMask(native.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        native.setHidesOnDeactivate(false);
        native.setCollectionBehavior(
            native.collectionBehavior()
                | NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn configure_usage_window(window: &tauri::WebviewWindow) -> Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    let handle = window.hwnd()?;
    unsafe {
        let current = GetWindowLongPtrW(handle, GWL_EXSTYLE);
        let desired = current | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize;
        SetWindowLongPtrW(handle, GWL_EXSTYLE, desired);
        SetWindowPos(
            handle,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        )
        .map_err(|error| ModelayError::Message(format!("应用 Windows 悬浮窗样式失败：{error}")))?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn configure_usage_window(_window: &tauri::WebviewWindow) -> Result<()> {
    Ok(())
}

pub fn codex_executable() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/ChatGPT.app/Contents/Resources/codex",
        ));
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join("Applications/ChatGPT.app/Contents/Resources/codex"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        // An explicit override is authoritative. Prefer user-local relocated
        // binaries next: Windows Store package files are commonly protected by
        // ACLs and cannot be launched by an unrelated desktop process.
        if let Some(path) = std::env::var_os("CODEX_CLI_PATH") {
            candidates.push(PathBuf::from(path));
        }
        append_windows_user_codex_candidates(
            &mut candidates,
            std::env::var_os("LOCALAPPDATA").as_deref().map(Path::new),
            dirs::home_dir().as_deref(),
            std::env::var_os("APPDATA").as_deref().map(Path::new),
        );
        // Prefer a user-installed PATH Codex before package directories. Check
        // both the native executable and npm's Windows command shim.
        if let Some(paths) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&paths) {
                candidates.push(directory.join("codex.exe"));
                candidates.push(directory.join("codex.cmd"));
            }
        }
        candidates.extend(windows_command_paths("codex.exe"));
        candidates.extend(windows_command_paths("codex.cmd"));
        // Finally let Windows resolve the App Execution Alias. A Store package
        // may expose this even when its real WindowsApps path is inaccessible.
        candidates.push(PathBuf::from("codex.exe"));
        if let Some(path) = running_windows_process_path("ChatGPT.exe") {
            if let Some(directory) = path.parent() {
                candidates.push(directory.join("resources/codex.exe"));
                candidates.push(directory.join("Resources/codex.exe"));
            }
        }
        if let Some(path) = running_windows_process_path("Codex.exe") {
            if let Some(directory) = path.parent() {
                candidates.push(directory.join("resources/codex.exe"));
                candidates.push(directory.join("Resources/codex.exe"));
            }
        }
        if let Some(location) = windows_chatgpt_package_location() {
            candidates.push(location.join("codex.exe"));
            candidates.push(location.join("resources/codex.exe"));
            candidates.push(location.join("Resources/codex.exe"));
            candidates.push(location.join("app/resources/codex.exe"));
            candidates.push(location.join("app/Resources/codex.exe"));
        }
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            candidates.push(directory.join(if cfg!(windows) { "codex.exe" } else { "codex" }));
        }
    }
    candidates.dedup();
    #[cfg(target_os = "windows")]
    let runnable = candidates
        .into_iter()
        .find(|path| windows_codex_is_runnable(path));
    #[cfg(not(target_os = "windows"))]
    let runnable = candidates.into_iter().find(|path| path.is_file());
    runnable.ok_or_else(|| {
        #[cfg(target_os = "windows")]
        let message = "找不到可运行的 Codex 命令行组件。请先启动一次 Codex Windows 应用；仍失败时，请安装独立 Codex CLI，或把可运行的 codex.exe 完整路径写入用户环境变量 CODEX_CLI_PATH，然后彻底退出并重开 Modelay。";
        #[cfg(not(target_os = "windows"))]
        let message = "找不到 Codex 命令行组件，请确认 ChatGPT/Codex 已安装。";
        ModelayError::Message(
            message.into(),
        )
    })
}

#[cfg(any(target_os = "windows", test))]
fn append_windows_user_codex_candidates(
    candidates: &mut Vec<PathBuf>,
    local_app_data: Option<&Path>,
    home: Option<&Path>,
    app_data: Option<&Path>,
) {
    if let Some(local) = local_app_data {
        let user_bin = local.join("OpenAI/Codex/bin");
        candidates.push(user_bin.join("codex.exe"));
        append_child_candidates(candidates, &user_bin, Path::new("codex.exe"));
        candidates.push(local.join("Microsoft/WindowsApps/codex.exe"));
        candidates.push(local.join("Programs/Codex/codex.exe"));
        candidates.push(local.join("Programs/Codex/resources/codex.exe"));
        candidates.push(local.join("Programs/OpenAI/Codex/codex.exe"));
        candidates.push(local.join("Programs/OpenAI/Codex/resources/codex.exe"));
        candidates.push(local.join("OpenAI/Codex/resources/codex.exe"));
        candidates.push(local.join("Programs/OpenAI/ChatGPT/resources/codex.exe"));
        candidates.push(local.join("OpenAI/ChatGPT/resources/codex.exe"));
    }
    if let Some(home) = home {
        append_child_candidates(
            candidates,
            &home.join(".codex/packages/standalone/releases"),
            Path::new("bin/codex.exe"),
        );
    }
    if let Some(app_data) = app_data {
        candidates.push(app_data.join("npm/codex.exe"));
        candidates.push(app_data.join("npm/codex.cmd"));
    }
}

#[cfg(any(target_os = "windows", test))]
fn append_child_candidates(candidates: &mut Vec<PathBuf>, root: &Path, suffix: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut matches = entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with(".staging-"))
        .map(|entry| entry.path().join(suffix))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    matches.sort_by_key(|path| {
        std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    matches.reverse();
    candidates.extend(matches);
}

#[cfg(target_os = "windows")]
fn windows_codex_is_runnable(path: &Path) -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    use wait_timeout::ChildExt;

    // Spawning is the only reliable ACL check for WindowsApps paths. Ignore
    // the exit code because some Codex builds return non-zero for --version.
    let mut child = match Command::new(path)
        .arg("--version")
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    match child.wait_timeout(std::time::Duration::from_secs(3)) {
        Ok(Some(_)) => true,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            true
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            true
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_command_paths(command: &str) -> Vec<PathBuf> {
    powershell_lines(&format!(
        "Get-Command '{command}' -All -CommandType Application -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source"
    ))
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

/// Remove provider credentials inherited by the Modelay process before launching
/// Codex or ChatGPT. The target channel is added back explicitly by the caller.
pub fn clear_provider_environment(command: &mut Command) {
    for (key, _) in std::env::vars() {
        if key == "AILINK_API_KEY" || (key.starts_with("CODEX_") && key.ends_with("_API_KEY")) {
            command.env_remove(key);
        }
    }
}

#[cfg(target_os = "macos")]
pub fn get_user_environment(key: &str) -> Result<Option<String>> {
    let output = Command::new("/bin/launchctl")
        .args(["getenv", key])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!value.is_empty()).then_some(value))
}

#[cfg(target_os = "macos")]
pub fn set_user_environment(key: &str, value: Option<&str>) -> Result<()> {
    let mut command = Command::new("/bin/launchctl");
    match value {
        Some(value) => {
            command.args(["setenv", key, value]);
        }
        None => {
            command.args(["unsetenv", key]);
        }
    }
    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ModelayError::Message(format!(
            "设置环境变量失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(target_os = "windows")]
pub fn get_user_environment(key: &str) -> Result<Option<String>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new("reg")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["query", r"HKCU\Environment", "/v", key])
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let value = text
        .lines()
        .find(|line| line.contains(key))
        .and_then(|line| line.split("REG_SZ").nth(1))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    Ok(value)
}

#[cfg(target_os = "windows")]
pub fn set_user_environment(key: &str, value: Option<&str>) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = match value {
        Some(value) => Command::new("reg")
            .creation_flags(CREATE_NO_WINDOW)
            .args([
                "add",
                r"HKCU\Environment",
                "/v",
                key,
                "/t",
                "REG_SZ",
                "/d",
                value,
                "/f",
            ])
            .output()?,
        None => Command::new("reg")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["delete", r"HKCU\Environment", "/v", key, "/f"])
            .output()?,
    };
    if output.status.success() {
        broadcast_environment_change();
        Ok(())
    } else if value.is_none() && get_user_environment(key)?.is_none() {
        // `reg delete` returns a failure status when the value is already absent.
        // Treat only a confirmed absence as success; other deletion failures must surface.
        broadcast_environment_change();
        Ok(())
    } else {
        Err(ModelayError::Message(
            "写入 Windows 用户环境变量失败。".into(),
        ))
    }
}

#[cfg(target_os = "windows")]
fn broadcast_environment_change() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    let label: Vec<u16> = "Environment\0".encode_utf16().collect();
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(label.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5000,
            None,
        );
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn get_user_environment(key: &str) -> Result<Option<String>> {
    Ok(std::env::var(key).ok())
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn set_user_environment(_key: &str, _value: Option<&str>) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn restart_chatgpt(environment: &[(String, Option<String>)]) -> Result<()> {
    // LaunchServices (`open -a`) does not reliably propagate provider
    // environment variables. Terminate the full process tree, then launch
    // ChatGPT's executable directly so the selected channel reaches Codex.
    let roots = Command::new("/usr/bin/pgrep")
        .args(["-x", "ChatGPT"])
        .output()
        .ok()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.trim().parse::<i32>().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for root in roots {
        let mut descendants = Vec::new();
        collect_descendants(root, &mut descendants);
        descendants.reverse();
        for pid in descendants.into_iter().chain(std::iter::once(root)) {
            let _ = Command::new("/bin/kill")
                .args(["-TERM", &pid.to_string()])
                .status();
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let mut candidates = vec![std::path::PathBuf::from(
        "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
    )];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Applications/ChatGPT.app/Contents/MacOS/ChatGPT"));
    }
    let executable = candidates
        .iter()
        .find(|path| path.is_file())
        .ok_or_else(|| ModelayError::Message("找不到 ChatGPT 应用程序。".into()))?;
    let mut command = Command::new(executable);
    clear_provider_environment(&mut command);
    for (key, value) in environment {
        command.env_remove(key);
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    // ChatGPT is a long-running GUI process. Waiting for its exit would leave
    // Modelay's restart action and every global button disabled indefinitely.
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(target_os = "macos")]
fn collect_descendants(parent: i32, result: &mut Vec<i32>) {
    let output = match Command::new("/usr/bin/pgrep")
        .args(["-P", &parent.to_string()])
        .output()
    {
        Ok(output) => output,
        Err(_) => return,
    };
    for child in String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
    {
        collect_descendants(child, result);
        result.push(child);
    }
}

#[cfg(target_os = "windows")]
pub fn restart_chatgpt(environment: &[(String, Option<String>)]) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let launch_target = windows_chatgpt_launch_target()?;
    let _ = Command::new("taskkill")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/IM", "ChatGPT.exe", "/F"])
        .status();
    std::thread::sleep(std::time::Duration::from_millis(1200));
    match launch_target {
        WindowsLaunchTarget::Executable(executable) => {
            let mut command = Command::new(executable);
            command.creation_flags(CREATE_NO_WINDOW);
            clear_provider_environment(&mut command);
            for (key, value) in environment {
                command.env_remove(key);
                if let Some(value) = value {
                    command.env(key, value);
                }
            }
            command.spawn()?;
        }
        WindowsLaunchTarget::AppId(app_id) => {
            let status = Command::new("explorer.exe")
                .creation_flags(CREATE_NO_WINDOW)
                .arg(format!(r"shell:AppsFolder\{app_id}"))
                .status()?;
            if !status.success() {
                return Err("无法通过 Windows 应用 ID 重新打开 ChatGPT。".into());
            }
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn restart_chatgpt(_environment: &[(String, Option<String>)]) -> Result<()> {
    Err("当前平台不支持重启 ChatGPT。".into())
}

#[cfg(target_os = "windows")]
fn chatgpt_candidates() -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Some(path) = running_windows_process_path("ChatGPT.exe") {
        result.push(path);
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        result.push(PathBuf::from(&local).join("Programs/OpenAI/ChatGPT/ChatGPT.exe"));
        result.push(PathBuf::from(&local).join("OpenAI/ChatGPT/ChatGPT.exe"));
    }
    if let Some(location) = windows_chatgpt_package_location() {
        result.push(location.join("ChatGPT.exe"));
        result.push(location.join("app/ChatGPT.exe"));
    }
    result
}

#[cfg(target_os = "windows")]
enum WindowsLaunchTarget {
    Executable(PathBuf),
    AppId(String),
}

#[cfg(target_os = "windows")]
fn windows_chatgpt_launch_target() -> Result<WindowsLaunchTarget> {
    if let Some(path) = chatgpt_candidates().into_iter().find(|path| path.is_file()) {
        return Ok(WindowsLaunchTarget::Executable(path));
    }
    let app_id = powershell_line(
        "Get-StartApps | Where-Object { $_.Name -eq 'ChatGPT' } | Select-Object -First 1 -ExpandProperty AppID",
    );
    app_id
        .map(WindowsLaunchTarget::AppId)
        .ok_or_else(|| ModelayError::Message("找不到 ChatGPT.exe 或 Windows 应用 ID。".into()))
}

#[cfg(target_os = "windows")]
fn running_windows_process_path(name: &str) -> Option<PathBuf> {
    let safe_name = name.replace('\'', "''");
    powershell_line(&format!(
        "Get-CimInstance Win32_Process -Filter \"Name='{safe_name}'\" | Select-Object -First 1 -ExpandProperty ExecutablePath"
    ))
    .map(PathBuf::from)
}

#[cfg(target_os = "windows")]
fn windows_chatgpt_package_location() -> Option<PathBuf> {
    powershell_line(
        "Get-AppxPackage | Where-Object { $_.Name -like '*ChatGPT*' -or $_.Name -like '*Codex*' -or $_.PackageFamilyName -like '*ChatGPT*' -or $_.PackageFamilyName -like '*Codex*' } | Sort-Object InstallDate -Descending | Select-Object -First 1 -ExpandProperty InstallLocation",
    )
    .map(PathBuf::from)
}

#[cfg(target_os = "windows")]
fn powershell_line(script: &str) -> Option<String> {
    powershell_lines(script).into_iter().next()
}

#[cfg(target_os = "windows")]
fn powershell_lines(script: &str) -> Vec<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(output) = Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn open_folder(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    let status = Command::new("/usr/bin/open").arg(path).status()?;
    #[cfg(target_os = "windows")]
    let status = {
        use std::os::windows::process::CommandExt;
        Command::new("explorer")
            .creation_flags(0x0800_0000)
            .arg(path)
            .status()?
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = Command::new("xdg-open").arg(path).status()?;
    if status.success() {
        Ok(())
    } else {
        Err("无法打开文件夹。".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_relocated_and_standalone_windows_codex_binaries() {
        let directory = tempfile::tempdir().unwrap();
        let local = directory.path().join("Local");
        let home = directory.path().join("Home");
        let app_data = directory.path().join("Roaming");
        let relocated = local.join("OpenAI/Codex/bin/current/codex.exe");
        let staging = local.join("OpenAI/Codex/bin/.staging-broken/codex.exe");
        let standalone = home.join(".codex/packages/standalone/releases/0.150/bin/codex.exe");
        let npm_shim = app_data.join("npm/codex.cmd");
        for path in [&relocated, &staging, &standalone, &npm_shim] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"fixture").unwrap();
        }

        let mut candidates = Vec::new();
        append_windows_user_codex_candidates(
            &mut candidates,
            Some(&local),
            Some(&home),
            Some(&app_data),
        );

        assert!(candidates.contains(&relocated));
        assert!(candidates.contains(&standalone));
        assert!(candidates.contains(&npm_shim));
        assert!(!candidates.contains(&staging));
    }
}
