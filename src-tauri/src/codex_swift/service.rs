use crate::app_config::AppType;
use crate::codex_swift::client;
use crate::codex_swift::fernet::decrypt_config_b64;
use crate::codex_swift::types::{
    CodexSwiftAccount, CodexSwiftActiveSession, CodexSwiftConfig, CodexSwiftGroup,
    CodexSwiftUsage,
};
use crate::error::AppError;
use crate::provider::{CodexSwiftProviderMeta, Provider, ProviderMeta};
use crate::services::ProviderService;
use crate::settings::{self, CodexSwiftSettings};
use crate::store::AppState;

const ACTIVE_SESSION_KEY: &str = "codex_swift_active_session";

fn is_http_unauthorized(err: &AppError) -> bool {
    matches!(err, AppError::Message(msg) if msg.starts_with("[HTTP 401]"))
}

fn require_credentials() -> Result<CodexSwiftSettings, AppError> {
    settings::get_codex_swift_settings().ok_or_else(|| {
        AppError::localized(
            "codex_swift.not_configured",
            "未配置 Codex Swift 账号",
            "Codex Swift account is not configured.",
        )
    })
}

pub async fn validate_and_login(base_url: &str, api_key: &str) -> Result<CodexSwiftAccount, AppError> {
    let base_url = base_url.trim();
    let api_key = api_key.trim();
    if base_url.is_empty() || api_key.is_empty() {
        return Err(AppError::InvalidInput(
            "base_url 和 api_key 不能为空".to_string(),
        ));
    }

    let account = client::fetch_me(base_url, api_key).await?;

    settings::set_codex_swift_settings(Some(CodexSwiftSettings {
        base_url: base_url.to_string(),
        api_key: api_key.to_string(),
    }))?;

    Ok(account)
}

pub async fn get_account() -> Result<Option<CodexSwiftAccount>, AppError> {
    let Some(creds) = settings::get_codex_swift_settings() else {
        return Ok(None);
    };
    if creds.base_url.is_empty() || creds.api_key.is_empty() {
        return Ok(None);
    }
    match client::fetch_me(&creds.base_url, &creds.api_key).await {
        Ok(account) => Ok(Some(account)),
        Err(e) if is_http_unauthorized(&e) => {
            // 401 视为登录已过期，静默清除本地凭据，前端切回登录态
            let _ = settings::set_codex_swift_settings(None);
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

pub async fn list_groups() -> Result<Vec<CodexSwiftGroup>, AppError> {
    let creds = require_credentials()?;
    client::fetch_groups(&creds.base_url, &creds.api_key).await
}

pub async fn get_usage() -> Result<CodexSwiftUsage, AppError> {
    let creds = require_credentials()?;
    client::fetch_usage(&creds.base_url, &creds.api_key).await
}

pub async fn apply_group(
    state: &AppState,
    group_id: &str,
    apps: Vec<String>,
) -> Result<CodexSwiftActiveSession, AppError> {
    let creds = require_credentials()?;

    let session = match client::create_session(&creds.base_url, &creds.api_key, group_id).await {
        Ok(s) => s,
        Err(e) if is_http_unauthorized(&e) => {
            // API key 已过期，清除本地状态，提示重新登录
            let _ = settings::set_codex_swift_settings(None);
            let _ = state.db.set_setting(ACTIVE_SESSION_KEY, "");
            return Err(AppError::localized(
                "codex_swift.session_expired",
                "登录已过期，请重新连接 Codex Swift",
                "Session expired, please reconnect Codex Swift.",
            ));
        }
        Err(e) => return Err(e),
    };
    let decrypted = decrypt_config_b64(&session.config_b64, None)?;

    let session_id_str = session.session_id.to_string();
    let group_name = session.group.name.clone();
    let group_multiplier = resolve_group_multiplier(&creds.base_url, &creds.api_key, group_id)
        .await
        .unwrap_or(session.group.multiplier)
        .to_string();

    for app_str in &apps {
        let app_type = parse_app_type(app_str)?;
        let provider = build_provider(
            group_id,
            &group_name,
            &session_id_str,
            &group_multiplier,
            &decrypted.url,
            &decrypted.key,
            &app_type,
        )?;
        let provider_id = provider.id.clone();
        // 若该分组供应商已存在则先删除（幂等写入）
        let _ = ProviderService::delete(state, app_type.clone(), &provider_id);
        ProviderService::add(state, app_type.clone(), provider)
            .map_err(|e| AppError::Message(format!("写入供应商失败: {e}")))?;
        // 切换至新供应商，使其立即生效
        ProviderService::switch(state, app_type, &provider_id)
            .map_err(|e| AppError::Message(format!("切换供应商失败: {e}")))?;
    }

    let active = CodexSwiftActiveSession {
        session_id: session_id_str,
        group_id: group_id.to_string(),
        group_name,
        balance: session.balance,
        started_at: session.started_at,
    };

    // 持久化 session 状态到 SQLite
    let json = serde_json::to_string(&active)
        .map_err(|e| AppError::Message(format!("session 序列化失败: {e}")))?;
    state.db.set_setting(ACTIVE_SESSION_KEY, &json)?;

    Ok(active)
}

pub fn get_active_session(state: &AppState) -> Result<Option<CodexSwiftActiveSession>, AppError> {
    match state.db.get_setting(ACTIVE_SESSION_KEY)? {
        Some(json) => {
            let session = serde_json::from_str::<CodexSwiftActiveSession>(&json)
                .map_err(|e| AppError::Message(format!("session 解析失败: {e}")))?;
            Ok(Some(session))
        }
        None => Ok(None),
    }
}

pub async fn logout(state: &AppState) -> Result<(), AppError> {
    // 立即清除本地凭据，UI 立刻切回登录态
    let creds = settings::get_codex_swift_settings();
    settings::set_codex_swift_settings(None)?;
    let _ = state.db.set_setting(ACTIVE_SESSION_KEY, "");

    // 后台通知服务端删除 session，不阻塞返回
    if let Some(creds) = creds {
        if !creds.base_url.is_empty() && !creds.api_key.is_empty() {
            tokio::spawn(async move {
                if let Err(e) =
                    client::delete_current_session(&creds.base_url, &creds.api_key).await
                {
                    log::warn!("[CodexSwift] background delete_session failed (ignored): {e}");
                }
            });
        }
    }

    Ok(())
}

pub fn get_config() -> Option<CodexSwiftConfig> {
    let creds = settings::get_codex_swift_settings()?;
    if creds.base_url.is_empty() {
        return None;
    }
    Some(CodexSwiftConfig {
        base_url: creds.base_url,
    })
}

// ── 内部帮助函数 ──────────────────────────────────────────────

fn parse_app_type(s: &str) -> Result<AppType, AppError> {
    match s {
        "claude" => Ok(AppType::Claude),
        "codex" => Ok(AppType::Codex),
        "gemini" => Ok(AppType::Gemini),
        "opencode" => Ok(AppType::OpenCode),
        "openclaw" => Ok(AppType::OpenClaw),
        "hermes" => Ok(AppType::Hermes),
        other => Err(AppError::InvalidInput(format!("未知 app 类型: {other}"))),
    }
}

async fn resolve_group_multiplier(
    base_url: &str,
    api_key: &str,
    group_id: &str,
) -> Result<f64, AppError> {
    let groups = client::fetch_groups(base_url, api_key).await?;
    Ok(groups
        .into_iter()
        .find(|group| group.id == group_id)
        .map(|group| group.multiplier)
        .unwrap_or(1.0))
}

fn build_provider(
    group_id: &str,
    group_name: &str,
    session_id: &str,
    cost_multiplier: &str,
    base_url: &str,
    api_key: &str,
    app_type: &AppType,
) -> Result<Provider, AppError> {
    let id = format!("codex-swift-{}-{}", group_id, app_type.as_str());
    let name = format!("[Codex Swift] {group_name}");

    let settings_config = match app_type {
        AppType::Claude => serde_json::json!({
            "env": {
                "ANTHROPIC_BASE_URL": base_url,
                "ANTHROPIC_AUTH_TOKEN": api_key,
            }
        }),
        AppType::Codex => {
            let base_trimmed = base_url.trim_end_matches('/');
            let origin_only = match base_trimmed.split_once("://") {
                Some((_scheme, rest)) => !rest.contains('/'),
                None => !base_trimmed.contains('/'),
            };
            let codex_base_url = if base_trimmed.ends_with("/v1") {
                base_trimmed.to_string()
            } else if origin_only {
                format!("{base_trimmed}/v1")
            } else {
                base_trimmed.to_string()
            };

            let config_toml = format!(
                r#"model_provider = "custom"
model = "gpt-5.4"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "CodexSwift"
base_url = "{codex_base_url}"
wire_api = "responses"
requires_openai_auth = true"#
            );
            serde_json::json!({
                "auth": { "OPENAI_API_KEY": api_key },
                "config": config_toml
            })
        }
        AppType::Gemini => serde_json::json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": base_url,
                "GEMINI_API_KEY": api_key,
            }
        }),
        _ => {
            return Err(AppError::InvalidInput(format!(
                "暂不支持应用类型 {}",
                app_type.as_str()
            )))
        }
    };

    let meta = ProviderMeta {
        codex_swift: Some(CodexSwiftProviderMeta {
            group_id: group_id.to_string(),
            session_id: session_id.to_string(),
        }),
        cost_multiplier: Some(cost_multiplier.to_string()),
        ..ProviderMeta::default()
    };

    Ok(Provider {
        id,
        name,
        settings_config,
        website_url: None,
        category: Some("aggregator".to_string()),
        created_at: Some(chrono::Utc::now().timestamp_millis()),
        sort_index: None,
        notes: None,
        meta: Some(meta),
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    })
}
