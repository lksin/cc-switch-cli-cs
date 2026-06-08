use std::time::Duration;

use crate::codex_swift::types::{
    CodexSwiftAccount, CodexSwiftGroup, CodexSwiftSessionResponse, CodexSwiftUsage,
};
use crate::error::AppError;

const TIMEOUT_SECS: u64 = 20;
const USER_AGENT: &str = "cc-switch/codex-swift-integration";

fn build_url(base_url: &str, path: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), path.trim_start_matches('/'))
}

async fn api_get<T: serde::de::DeserializeOwned>(
    base_url: &str,
    api_key: &str,
    path: &str,
) -> Result<T, AppError> {
    let client = crate::proxy::http_client::get();
    let url = build_url(base_url, path);

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| AppError::Message(format!("网络错误: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| body.clone());
        return Err(AppError::Message(format!("[HTTP {}] {}", status.as_u16(), msg)));
    }

    resp.json::<T>()
        .await
        .map_err(|e| AppError::Message(format!("响应解析失败: {e}")))
}

async fn api_post<B: serde::Serialize, T: serde::de::DeserializeOwned>(
    base_url: &str,
    api_key: &str,
    path: &str,
    body: &B,
) -> Result<T, AppError> {
    let client = crate::proxy::http_client::get();
    let url = build_url(base_url, path);

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .json(body)
        .send()
        .await
        .map_err(|e| AppError::Message(format!("网络错误: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&body_text)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| body_text.clone());
        return Err(AppError::Message(format!("[HTTP {}] {}", status.as_u16(), msg)));
    }

    resp.json::<T>()
        .await
        .map_err(|e| AppError::Message(format!("响应解析失败: {e}")))
}

pub async fn fetch_me(base_url: &str, api_key: &str) -> Result<CodexSwiftAccount, AppError> {
    api_get(base_url, api_key, "api/v2/me").await
}

pub async fn fetch_groups(base_url: &str, api_key: &str) -> Result<Vec<CodexSwiftGroup>, AppError> {
    api_get(base_url, api_key, "api/v2/groups").await
}

pub async fn fetch_usage(base_url: &str, api_key: &str) -> Result<CodexSwiftUsage, AppError> {
    api_get(base_url, api_key, "api/v2/me/usage").await
}

pub async fn create_session(
    base_url: &str,
    api_key: &str,
    group_id: &str,
) -> Result<CodexSwiftSessionResponse, AppError> {
    api_post(base_url, api_key, "api/v2/sessions", &serde_json::json!({ "group_id": group_id })).await
}

pub async fn delete_current_session(base_url: &str, api_key: &str) -> Result<(), AppError> {
    let client = crate::proxy::http_client::get();
    let url = build_url(base_url, "api/v2/sessions/current");

    let resp = client
        .delete(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| AppError::Message(format!("网络错误: {e}")))?;

    let status = resp.status();
    // 404 表示无活跃 session，忽略
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(());
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Message(format!("[HTTP {}] {}", status.as_u16(), body)));
    }
    Ok(())
}
