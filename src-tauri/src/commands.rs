use crate::config;
use crate::error::{command_error, ModelayError, Result};
use crate::models::*;
use crate::{codex, paths, platform, secrets, sessions, storage, usage};
use std::collections::HashSet;
use std::sync::Mutex;
use tauri::Manager;

const IMAGE_ENVIRONMENT_KEY: &str = "CODEX_SWITCH_IMAGE_SKILL";
static MUTATION_LOCK: Mutex<()> = Mutex::new(());

#[tauri::command]
pub async fn get_app_state() -> std::result::Result<AppState, String> {
    run_blocking(app_state).await
}

#[tauri::command]
pub async fn get_widget_state() -> std::result::Result<WidgetState, String> {
    run_blocking(widget_state).await
}

async fn run_blocking<T, F>(task: F) -> std::result::Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("后台任务异常结束：{error}"))?
        .map_err(command_error)
}

fn app_state() -> Result<AppState> {
    let preferences = storage::load_preferences()?;
    let config = config::read()?;
    let provider = config::active_provider(&config.document);
    let model = config::active_model(&config.document);
    let current_channel = preferences
        .channels
        .iter()
        .find(|channel| channel.provider_id() == provider);
    let current_mode =
        if provider.starts_with("openai") || config.document.get("model_provider").is_none() {
            "official"
        } else if current_channel.is_some() {
            "channel"
        } else {
            "unknown"
        };
    let mut channels = preferences.channels.clone();
    for channel in &mut channels {
        channel.has_secret = Some(secrets::has(channel));
    }
    let conformant = if current_mode == "official" {
        config.document.get("model_provider").is_none()
    } else {
        current_channel
            .map(|channel| config::is_channel_conformant(&config.document, channel))
            .unwrap_or(false)
    };
    Ok(AppState {
        platform: platform::platform_label(),
        current_mode: current_mode.into(),
        current_channel_id: current_channel.map(|channel| channel.id.clone()),
        current_provider_id: provider,
        current_model: model,
        official_logged_in: codex::login_status(),
        config_exists: config.existed,
        config_conformant: conformant,
        image_skill: storage::load_image_skill(),
        channels,
        official_model: preferences.official_model,
        backup_directory: paths::backup_dir()?.display().to_string(),
        dock_mode: preferences.dock_mode,
        widget_position: preferences.widget_position,
    })
}

fn widget_state() -> Result<WidgetState> {
    let preferences = storage::load_preferences()?;
    let config = config::read()?;
    let provider = config::active_provider(&config.document);
    let current_channel = preferences
        .channels
        .iter()
        .find(|channel| channel.provider_id() == provider);
    let current_mode =
        if provider.starts_with("openai") || config.document.get("model_provider").is_none() {
            "official"
        } else if current_channel.is_some() {
            "channel"
        } else {
            "unknown"
        };
    Ok(WidgetState {
        current_mode: current_mode.into(),
        current_channel_id: current_channel.map(|channel| channel.id.clone()),
        current_provider_id: provider,
        dock_mode: preferences.dock_mode,
    })
}

#[tauri::command]
pub async fn save_channel(request: SaveChannelRequest) -> std::result::Result<AppState, String> {
    run_blocking(move || save_channel_inner(request)).await
}

fn save_channel_inner(request: SaveChannelRequest) -> Result<AppState> {
    let _guard = lock_mutations()?;
    let mut channel = request.channel;
    let requested_secret = request.secret.and_then(normalize_secret);
    channel.name = channel.name.trim().to_owned();
    channel.base_url = channel.normalized_base_url();
    channel.model = channel.model.trim().to_owned();
    if channel.name.is_empty() || channel.model.is_empty() {
        return Err("渠道名称和模型不能为空。".into());
    }
    let url = url::Url::parse(&channel.base_url)
        .map_err(|_| ModelayError::Message("API 地址无效。".into()))?;
    let secure_or_local = url.scheme() == "https"
        || (url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1")));
    if !secure_or_local || url.host_str().is_none() {
        return Err("第三方 API 必须使用 HTTPS；仅本机 localhost 可使用 HTTP。".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("API 地址不能包含账号、密码、查询参数或片段。".into());
    }
    if !channel.has_valid_id() {
        return Err(
            "渠道 ID 只能由字母、数字、连字符和下划线组成，并且必须以字母或数字开头。".into(),
        );
    }
    channel.is_built_in = channel.id == "ailink";
    channel.has_secret = None;
    let active_config = config::read()?;
    let is_active = config::active_provider(&active_config.document) == channel.provider_id();
    let environment_key = channel.environment_key();
    let previous_environment = platform::get_user_environment(&environment_key)?;
    let previous_secret = if requested_secret.is_some() {
        secrets::stored(&channel)?
    } else {
        None
    };
    let mut preferences = storage::load_preferences()?;
    let previous_preferences = preferences.clone();
    if preferences.channels.iter().any(|existing| {
        existing.id != channel.id && existing.provider_id() == channel.provider_id()
    }) {
        return Err(ModelayError::Message(format!(
            "渠道 ID {} 与已有渠道生成了相同的 Provider，请更换 ID。",
            channel.id
        )));
    }
    if let Some(index) = preferences
        .channels
        .iter()
        .position(|item| item.id == channel.id)
    {
        preferences.channels[index] = channel.clone();
    } else {
        preferences.channels.push(channel.clone());
    }
    preferences.last_channel_id = Some(channel.id.clone());
    storage::save_preferences(&preferences)?;
    if let Some(secret) = requested_secret.as_deref() {
        if let Err(error) = secrets::set(&channel, secret) {
            return Err(with_rollback_context(
                error,
                vec![
                    (
                        "系统凭据",
                        restore_secret(&channel, previous_secret.as_deref()),
                    ),
                    ("渠道偏好", storage::save_preferences(&previous_preferences)),
                ],
            ));
        }
        let environment_value = is_active.then_some(secret);
        if let Err(error) = platform::set_user_environment(&environment_key, environment_value) {
            return Err(with_rollback_context(
                error,
                vec![
                    (
                        "系统凭据",
                        restore_secret(&channel, previous_secret.as_deref()),
                    ),
                    (
                        "环境变量",
                        platform::set_user_environment(
                            &environment_key,
                            previous_environment.as_deref(),
                        ),
                    ),
                    ("渠道偏好", storage::save_preferences(&previous_preferences)),
                ],
            ));
        }
    }
    match app_state() {
        Ok(state) => Ok(state),
        Err(error) => {
            let mut rollback = vec![("渠道偏好", storage::save_preferences(&previous_preferences))];
            if requested_secret.is_some() {
                rollback.push((
                    "系统凭据",
                    restore_secret(&channel, previous_secret.as_deref()),
                ));
                rollback.push((
                    "环境变量",
                    platform::set_user_environment(
                        &environment_key,
                        previous_environment.as_deref(),
                    ),
                ));
            }
            Err(with_rollback_context(error, rollback))
        }
    }
}

#[tauri::command]
pub async fn delete_channel(channel_id: String) -> std::result::Result<AppState, String> {
    run_blocking(move || {
        let _guard = lock_mutations()?;
        let mut preferences = storage::load_preferences()?;
        let previous_preferences = preferences.clone();
        let channel = preferences
            .channels
            .iter()
            .find(|channel| channel.id == channel_id)
            .cloned()
            .ok_or_else(|| ModelayError::Message("找不到该渠道。".into()))?;
        if channel.is_built_in {
            return Err("内置 AiLink 渠道不能删除。".into());
        }
        let active = config::read()?;
        if config::active_provider(&active.document) == channel.provider_id() {
            return Err("当前渠道正在使用中，请先切换到其他渠道再删除。".into());
        }
        let environment_key = channel.environment_key();
        let previous_environment = platform::get_user_environment(&environment_key)?;
        let previous_secret = secrets::stored(&channel)?;
        preferences.channels.retain(|item| item.id != channel_id);
        if preferences.last_channel_id.as_deref() == Some(channel_id.as_str()) {
            preferences.last_channel_id = Some("ailink".into());
        }
        storage::save_preferences(&preferences)?;
        if let Err(error) = platform::set_user_environment(&environment_key, None) {
            return Err(with_rollback_context(
                error,
                vec![("渠道偏好", storage::save_preferences(&previous_preferences))],
            ));
        }
        if let Err(error) = secrets::delete(&channel) {
            return Err(with_rollback_context(
                error,
                vec![
                    (
                        "系统凭据",
                        restore_secret(&channel, previous_secret.as_deref()),
                    ),
                    (
                        "环境变量",
                        platform::set_user_environment(
                            &environment_key,
                            previous_environment.as_deref(),
                        ),
                    ),
                    ("渠道偏好", storage::save_preferences(&previous_preferences)),
                ],
            ));
        }
        match app_state() {
            Ok(state) => Ok(state),
            Err(error) => Err(with_rollback_context(
                error,
                vec![
                    (
                        "系统凭据",
                        restore_secret(&channel, previous_secret.as_deref()),
                    ),
                    (
                        "环境变量",
                        platform::set_user_environment(
                            &environment_key,
                            previous_environment.as_deref(),
                        ),
                    ),
                    ("渠道偏好", storage::save_preferences(&previous_preferences)),
                ],
            )),
        }
    })
    .await
}

#[tauri::command]
pub async fn save_secret(
    channel_id: String,
    secret: String,
) -> std::result::Result<AppState, String> {
    run_blocking(move || {
        let _guard = lock_mutations()?;
        let secret = normalize_secret(secret)
            .ok_or_else(|| ModelayError::Message("API 密钥不能为空。".into()))?;
        let preferences = storage::load_preferences()?;
        let channel = find_channel(&preferences, &channel_id)?.clone();
        let previous_secret = secrets::stored(&channel)?;
        let environment_key = channel.environment_key();
        let previous_environment = platform::get_user_environment(&environment_key)?;
        let active = config::active_provider(&config::read()?.document) == channel.provider_id();
        secrets::set(&channel, &secret)?;
        let environment_value = active.then_some(secret.as_str());
        if let Err(error) = platform::set_user_environment(&environment_key, environment_value) {
            return Err(with_rollback_context(
                error,
                vec![
                    (
                        "系统凭据",
                        restore_secret(&channel, previous_secret.as_deref()),
                    ),
                    (
                        "环境变量",
                        platform::set_user_environment(
                            &environment_key,
                            previous_environment.as_deref(),
                        ),
                    ),
                ],
            ));
        }
        match app_state() {
            Ok(state) => Ok(state),
            Err(error) => Err(with_rollback_context(
                error,
                vec![
                    (
                        "系统凭据",
                        restore_secret(&channel, previous_secret.as_deref()),
                    ),
                    (
                        "环境变量",
                        platform::set_user_environment(
                            &environment_key,
                            previous_environment.as_deref(),
                        ),
                    ),
                ],
            )),
        }
    })
    .await
}

#[tauri::command]
pub async fn delete_secret(channel_id: String) -> std::result::Result<AppState, String> {
    run_blocking(move || {
        let _guard = lock_mutations()?;
        let preferences = storage::load_preferences()?;
        let channel = find_channel(&preferences, &channel_id)?.clone();
        let previous_secret = secrets::stored(&channel)?;
        let environment_key = channel.environment_key();
        let previous_environment = platform::get_user_environment(&environment_key)?;
        secrets::delete(&channel)?;
        if let Err(error) = platform::set_user_environment(&environment_key, None) {
            return Err(with_rollback_context(
                error,
                vec![
                    (
                        "系统凭据",
                        restore_secret(&channel, previous_secret.as_deref()),
                    ),
                    (
                        "环境变量",
                        platform::set_user_environment(
                            &environment_key,
                            previous_environment.as_deref(),
                        ),
                    ),
                ],
            ));
        }
        match app_state() {
            Ok(state) => Ok(state),
            Err(error) => Err(with_rollback_context(
                error,
                vec![
                    (
                        "系统凭据",
                        restore_secret(&channel, previous_secret.as_deref()),
                    ),
                    (
                        "环境变量",
                        platform::set_user_environment(
                            &environment_key,
                            previous_environment.as_deref(),
                        ),
                    ),
                ],
            )),
        }
    })
    .await
}

#[tauri::command]
pub async fn list_models(channel_id: String) -> std::result::Result<Vec<ModelInfo>, String> {
    run_blocking(move || {
        if channel_id == "official" {
            return codex::list_models();
        }
        let preferences = storage::load_preferences()?;
        let channel = find_channel(&preferences, &channel_id)?;
        let secret = secrets::get(channel)?.ok_or_else(|| {
            ModelayError::Message(format!("尚未保存 {} 的 API 密钥。", channel.name))
        })?;
        usage::list_channel_models(channel, &secret)
    })
    .await
}

#[tauri::command]
pub async fn login_official() -> std::result::Result<AppState, String> {
    run_blocking(|| codex::login().and_then(|_| app_state())).await
}

#[tauri::command]
pub async fn switch_channel(request: SwitchRequest) -> std::result::Result<SwitchReport, String> {
    run_blocking(move || switch_inner(request)).await
}

fn switch_inner(request: SwitchRequest) -> Result<SwitchReport> {
    let _guard = lock_mutations()?;
    let mut preferences = storage::load_preferences()?;
    let previous_preferences = preferences.clone();
    let is_official = request.channel_id == "official";
    let model = request.model.trim();
    if model.is_empty() {
        return Err("模型不能为空。".into());
    }
    let channel = if is_official {
        None
    } else {
        Some(find_channel(&preferences, &request.channel_id)?.clone())
    };
    let secret = match &channel {
        Some(channel) => Some(secrets::get(channel)?.ok_or_else(|| {
            ModelayError::Message(format!("尚未保存 {} 的 API 密钥。", channel.name))
        })?),
        None => None,
    };
    if is_official {
        if !codex::login_status() {
            return Err("当前不是 ChatGPT 官方账号登录，请先完成官方登录。".into());
        }
        let available = codex::list_models()?;
        ensure_model_supported(
            available.iter().map(|item| item.id.as_str()),
            model,
            "官方账号",
        )?;
    }
    if let (Some(channel), Some(secret)) = (&channel, &secret) {
        if channel.validates_model_list {
            let supported: HashSet<_> = usage::list_channel_models(channel, secret)?
                .into_iter()
                .map(|model| model.id)
                .collect();
            ensure_model_supported(supported.iter().map(String::as_str), model, &channel.name)?;
        }
    }
    let mut config_document = config::read()?;
    let backup_path = config::backup(&config_document.original)?;
    let environment_key = channel.as_ref().map(ChannelProfile::environment_key);
    let mut environment_keys = preferences
        .channels
        .iter()
        .map(ChannelProfile::environment_key)
        .collect::<Vec<_>>();
    environment_keys.sort();
    environment_keys.dedup();
    let previous_environments = environment_keys
        .iter()
        .map(|key| Ok((key.clone(), platform::get_user_environment(key)?)))
        .collect::<Result<Vec<_>>>()?;
    let previous_image_environment = platform::get_user_environment(IMAGE_ENVIRONMENT_KEY)?;
    let previous_skill_file = storage::snapshot_image_skill()?;
    let session_backup = sessions::backup()?;
    let image_skill = if is_official { "imagegen" } else { "imagegen2" };
    let result = (|| -> Result<SwitchReport> {
        let provider_id = if is_official {
            config::activate_official(&mut config_document.document, model);
            sessions::detect_official_provider()
        } else {
            let mut selected = channel.clone().unwrap();
            selected.model = model.into();
            config::activate_channel(&mut config_document.document, &selected)?;
            selected.provider_id()
        };
        config::write(&config_document.document)?;
        for key in &environment_keys {
            let value = if environment_key.as_deref() == Some(key.as_str()) {
                secret.as_deref()
            } else {
                None
            };
            platform::set_user_environment(key, value)?;
        }
        platform::set_user_environment(IMAGE_ENVIRONMENT_KEY, Some(image_skill))?;
        storage::save_image_skill(image_skill)?;
        let mut checks = vec![CheckResult {
            title: "配置文件".into(),
            detail: "Provider、模型与认证方式已原子写入".into(),
            state: CheckState::Passed,
        }];
        let doctor_environment = environment_keys
            .iter()
            .map(|key| {
                let value = if environment_key.as_deref() == Some(key.as_str()) {
                    secret.as_deref()
                } else {
                    None
                };
                (key.as_str(), value)
            })
            .collect::<Vec<_>>();
        let doctor_detail = codex::doctor(&doctor_environment)?;
        checks.push(CheckResult {
            title: "Codex Doctor".into(),
            detail: doctor_detail,
            state: CheckState::Passed,
        });
        if is_official {
            checks.push(CheckResult {
                title: "官方登录与模型".into(),
                detail: format!("ChatGPT 登录有效，模型 {model} 可用"),
                state: CheckState::Passed,
            });
            checks.push(CheckResult {
                title: "渠道环境变量".into(),
                detail: "已清除所有第三方渠道的启动环境变量".into(),
                state: CheckState::Passed,
            });
        } else {
            let status =
                usage::endpoint_status(channel.as_ref().unwrap(), secret.as_ref().unwrap())?;
            checks.push(CheckResult {
                title: format!("{} 服务", channel.as_ref().unwrap().name),
                detail: status,
                state: CheckState::Passed,
            });
            checks.push(CheckResult {
                title: "密钥注入".into(),
                detail: "系统凭据库 + 环境变量，config.toml 不含明文密钥".into(),
                state: CheckState::Passed,
            });
        }
        if is_official {
            preferences.official_model = model.into();
        } else if let Some(item) = preferences
            .channels
            .iter_mut()
            .find(|item| item.id == request.channel_id)
        {
            item.model = model.into();
        }
        preferences.last_channel_id = (!is_official).then_some(request.channel_id.clone());
        storage::save_preferences(&preferences)?;
        if let Some(report) =
            sessions::rebind_prepared(session_backup.as_ref(), &provider_id, model)?
        {
            checks.push(CheckResult {
                title: "全部旧任务".into(),
                detail: format!(
                    "已覆盖 {} 个用户任务为 {} / {}",
                    report.changed_count, provider_id, model
                ),
                state: CheckState::Passed,
            });
            checks.push(CheckResult {
                title: "任务索引备份".into(),
                detail: report.backup_path.display().to_string(),
                state: CheckState::Passed,
            });
        } else {
            checks.push(CheckResult {
                title: "全部旧任务".into(),
                detail: "未找到任务索引；新任务仍使用当前渠道".into(),
                state: CheckState::Warning,
            });
        }
        checks.push(CheckResult {
            title: "配置备份".into(),
            detail: backup_path.display().to_string(),
            state: CheckState::Passed,
        });
        Ok(SwitchReport {
            channel_id: request.channel_id.clone(),
            provider_id,
            model: model.into(),
            image_skill: image_skill.into(),
            backup_path: backup_path.display().to_string(),
            needs_restart: true,
            checks,
        })
    })();
    match result {
        Ok(report) => Ok(report),
        Err(error) => {
            let mut rollback = vec![(
                "Codex 配置",
                config::restore(&config_document.original, config_document.existed),
            )];
            for (key, value) in &previous_environments {
                rollback.push((
                    "渠道环境变量",
                    platform::set_user_environment(key, value.as_deref()),
                ));
            }
            rollback.push((
                "生图环境变量",
                platform::set_user_environment(
                    IMAGE_ENVIRONMENT_KEY,
                    previous_image_environment.as_deref(),
                ),
            ));
            rollback.push(("生图路由", previous_skill_file.restore()));
            rollback.push(("渠道偏好", storage::save_preferences(&previous_preferences)));
            Err(with_rollback_context(error, rollback))
        }
    }
}

#[tauri::command]
pub async fn restart_chatgpt() -> std::result::Result<(), String> {
    run_blocking(|| {
        let state = app_state()?;
        let preferences = storage::load_preferences()?;
        let mut environment = preferences
            .channels
            .iter()
            .map(|channel| (channel.environment_key(), None))
            .collect::<Vec<(String, Option<String>)>>();
        environment.push((
            IMAGE_ENVIRONMENT_KEY.into(),
            Some(state.image_skill.clone()),
        ));
        if state.current_mode == "channel" {
            let channel = state
                .current_channel_id
                .as_deref()
                .and_then(|id| preferences.channels.iter().find(|channel| channel.id == id))
                .ok_or_else(|| ModelayError::Message("无法识别当前渠道。".into()))?;
            let secret = secrets::get(channel)?
                .ok_or_else(|| ModelayError::Message("当前渠道缺少 API 密钥。".into()))?;
            let key = channel.environment_key();
            if let Some((_, value)) = environment
                .iter_mut()
                .find(|(environment_key, _)| environment_key == &key)
            {
                *value = Some(secret);
            } else {
                environment.push((key, Some(secret)));
            }
        }
        environment.sort_by(|left, right| left.0.cmp(&right.0));
        environment.dedup_by(|left, right| left.0 == right.0);
        platform::restart_chatgpt(&environment)
    })
    .await
}

#[tauri::command]
pub async fn open_backup_folder() -> std::result::Result<(), String> {
    run_blocking(|| platform::open_folder(&paths::backup_dir()?)).await
}

#[tauri::command]
pub async fn get_usage(channel_id: String) -> std::result::Result<UsageSnapshot, String> {
    run_blocking(move || {
        if channel_id == "official" {
            return usage::official();
        }
        let preferences = storage::load_preferences()?;
        let channel = find_channel(&preferences, &channel_id)?;
        let secret = secrets::get(channel)?.ok_or_else(|| {
            ModelayError::Message(format!("尚未保存 {} 的 API 密钥。", channel.name))
        })?;
        usage::channel(channel, &secret)
    })
    .await
}

#[tauri::command]
pub async fn set_widget_mode(
    app: tauri::AppHandle,
    mode: String,
) -> std::result::Result<AppState, String> {
    if !matches!(mode.as_str(), "free" | "edge" | "off") {
        return Err("无效的悬浮窗模式。".into());
    }
    let saved_mode = mode.clone();
    let (previous_mode, saved_position) = run_blocking(move || {
        let _guard = lock_mutations()?;
        let mut preferences = storage::load_preferences()?;
        let previous_mode = preferences.dock_mode.clone();
        preferences.dock_mode = saved_mode;
        let saved_position = preferences.widget_position;
        storage::save_preferences(&preferences)?;
        Ok((previous_mode, saved_position))
    })
    .await?;
    if let Some(window) = app.get_webview_window("usage") {
        let window_result = if mode == "off" {
            window.hide()
        } else {
            let position_result = if mode == "free" {
                saved_position
                    .map(|position| {
                        window.set_position(tauri::Position::Physical(
                            tauri::PhysicalPosition::new(position.x, position.y),
                        ))
                    })
                    .unwrap_or(Ok(()))
            } else {
                Ok(())
            };
            position_result.and_then(|_| window.show())
        };
        if let Err(error) = window_result {
            let _ = run_blocking(move || {
                let _guard = lock_mutations()?;
                let mut preferences = storage::load_preferences()?;
                preferences.dock_mode = previous_mode;
                storage::save_preferences(&preferences)
            })
            .await;
            return Err(command_error(error.into()));
        }
    }
    run_blocking(app_state).await
}

#[tauri::command]
pub async fn save_widget_position(x: i32, y: i32) -> std::result::Result<(), String> {
    run_blocking(move || {
        let _guard = lock_mutations()?;
        let mut preferences = storage::load_preferences()?;
        preferences.widget_position = Some(WidgetPosition { x, y });
        storage::save_preferences(&preferences)
    })
    .await
}

fn find_channel<'a>(preferences: &'a Preferences, channel_id: &str) -> Result<&'a ChannelProfile> {
    preferences
        .channels
        .iter()
        .find(|channel| channel.id == channel_id)
        .ok_or_else(|| ModelayError::Message(format!("找不到渠道 {channel_id}。")))
}

fn restore_secret(channel: &ChannelProfile, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => secrets::set(channel, value),
        None => secrets::delete(channel),
    }
}

fn normalize_secret(secret: String) -> Option<String> {
    let secret = secret.trim();
    (!secret.is_empty()).then(|| secret.to_owned())
}

fn with_rollback_context(
    error: ModelayError,
    rollback: Vec<(&'static str, Result<()>)>,
) -> ModelayError {
    let failures = rollback
        .into_iter()
        .filter_map(|(label, result)| result.err().map(|error| format!("{label}: {error}")))
        .collect::<Vec<_>>();
    if failures.is_empty() {
        error
    } else {
        ModelayError::Message(format!(
            "{error}；自动回滚未完全成功：{}。请不要重启 ChatGPT，并从备份目录恢复。",
            failures.join("；")
        ))
    }
}

fn lock_mutations() -> Result<std::sync::MutexGuard<'static, ()>> {
    MUTATION_LOCK
        .lock()
        .map_err(|_| ModelayError::Message("渠道修改锁异常，请重新启动 Modelay。".into()))
}

fn ensure_model_supported<'a>(
    available: impl IntoIterator<Item = &'a str>,
    model: &str,
    channel_name: &str,
) -> Result<()> {
    if available.into_iter().any(|candidate| candidate == model) {
        Ok(())
    } else {
        Err(ModelayError::Message(format!(
            "{channel_name} 不支持模型 {model}。"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_unavailable_model_before_switching() {
        assert!(ensure_model_supported(["gpt-a", "gpt-b"], "gpt-b", "Test").is_ok());
        assert!(ensure_model_supported(["gpt-a", "gpt-b"], "gpt-c", "Test")
            .unwrap_err()
            .to_string()
            .contains("不支持模型"));
    }

    #[test]
    fn normalizes_secrets_before_storing_or_injecting_them() {
        assert_eq!(
            normalize_secret("  test-secret  ".into()).as_deref(),
            Some("test-secret")
        );
        assert_eq!(normalize_secret("   ".into()), None);
    }

    #[test]
    fn rollback_failures_are_never_silently_discarded() {
        let error = with_rollback_context(
            ModelayError::Message("切换失败".into()),
            vec![
                ("配置", Ok(())),
                ("环境变量", Err(ModelayError::Message("拒绝访问".into()))),
            ],
        );
        let message = error.to_string();
        assert!(message.contains("切换失败"));
        assert!(message.contains("自动回滚未完全成功"));
        assert!(message.contains("环境变量"));
    }
}
