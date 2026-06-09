use crate::codex_swift::service;
use crate::codex_swift::types::CodexSwiftActiveSession;
use crate::error::AppError;
use crate::settings;

use super::super::app::{CodexSwiftTuiState, ToastKind};
use super::super::data::{load_state, UiData};
use super::RuntimeActionContext;

fn create_runtime() -> Result<tokio::runtime::Runtime, AppError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::Message(format!("创建 tokio 运行时失败: {e}")))
}

pub(super) fn refresh(ctx: &mut RuntimeActionContext<'_>) -> Result<(), AppError> {
    ctx.app.codex_swift_state.loading = true;
    ctx.app.codex_swift_state.error = None;

    let rt = create_runtime()?;

    // Load account
    let account_result = rt.block_on(service::get_account());
    // Load groups (only if credentials exist)
    let groups_result = if settings::get_codex_swift_settings().is_some() {
        rt.block_on(service::list_groups()).ok().unwrap_or_default()
    } else {
        vec![]
    };
    // Load active session (sync, no async needed)
    let state = load_state()?;
    let active_session = service::get_active_session(&state).ok().flatten();

    ctx.app.codex_swift_state.loading = false;
    match account_result {
        Ok(account) => {
            ctx.app.codex_swift_state.account = account;
            ctx.app.codex_swift_state.groups = groups_result;
            ctx.app.codex_swift_state.active_session = active_session;
        }
        Err(e) => {
            ctx.app.codex_swift_state.account = None;
            ctx.app.codex_swift_state.error = Some(e.to_string());
        }
    }
    Ok(())
}

pub(super) fn login(
    ctx: &mut RuntimeActionContext<'_>,
    base_url: String,
    api_key: String,
) -> Result<(), AppError> {
    ctx.app.codex_swift_state.loading = true;
    ctx.app.codex_swift_state.error = None;

    let rt = create_runtime()?;
    let result = rt.block_on(service::validate_and_login(&base_url, &api_key));

    ctx.app.codex_swift_state.loading = false;
    match result {
        Ok(account) => {
            let username = account.username.clone();
            ctx.app.codex_swift_state.account = Some(account);
            let en = format!("Connected to Codex Swift as {username}");
            let zh = format!("已连接 Codex Swift，欢迎 {username}");
            ctx.app.push_toast(crate::t!(&en, &zh), ToastKind::Success);
            let groups = rt.block_on(service::list_groups()).unwrap_or_default();
            ctx.app.codex_swift_state.groups = groups;
            ctx.app.codex_swift_groups_idx = 0;
        }
        Err(e) => {
            ctx.app.codex_swift_state.error = Some(e.to_string());
            let en = format!("Login failed: {e}");
            let zh = format!("登录失败: {e}");
            ctx.app.push_toast(crate::t!(&en, &zh), ToastKind::Error);
        }
    }
    Ok(())
}

pub(super) fn logout(ctx: &mut RuntimeActionContext<'_>) -> Result<(), AppError> {
    let state = load_state()?;
    let rt = create_runtime()?;
    rt.block_on(service::logout(&state))?;

    ctx.app.codex_swift_state = CodexSwiftTuiState::default();
    ctx.app.push_toast(
        crate::t!("Disconnected from Codex Swift.", "已断开 Codex Swift 连接。"),
        ToastKind::Success,
    );
    Ok(())
}

pub(super) fn apply_group(
    ctx: &mut RuntimeActionContext<'_>,
    group_id: String,
    apps: Vec<String>,
) -> Result<(), AppError> {
    let state = load_state()?;

    if apps.is_empty() {
        ctx.app.push_toast(
            crate::t!(
                "No apps selected.",
                "未选择任何应用。"
            ),
            ToastKind::Warning,
        );
        return Ok(());
    }

    let rt = create_runtime()?;
    match rt.block_on(service::apply_group(&state, &group_id, apps)) {
        Ok(session) => {
            let group_name = session.group_name.clone();
            ctx.app.codex_swift_state.active_session = Some(CodexSwiftActiveSession {
                session_id: session.session_id,
                group_id: session.group_id,
                group_name: group_name.clone(),
                balance: session.balance,
                started_at: session.started_at,
            });
            *ctx.data = UiData::load(&ctx.app.app_type)?;
            let en = format!("Applied group '{group_name}' to providers.");
            let zh = format!("已应用群组「{group_name}」的供应商配置。");
            ctx.app.push_toast(crate::t!(&en, &zh), ToastKind::Success);
        }
        Err(e) => {
            let en = format!("Apply failed: {e}");
            let zh = format!("应用失败: {e}");
            ctx.app.push_toast(crate::t!(&en, &zh), ToastKind::Error);
        }
    }
    Ok(())
}
