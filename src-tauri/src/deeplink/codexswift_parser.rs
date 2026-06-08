//! Codex Swift deep link URL parser
//!
//! Supported URL formats:
//!   codexswift://v1/login?baseUrl=xxx&apiKey=xxx
//!   codexswift://v1/import-group?groupId=xxx

use super::CodexSwiftDeepLinkRequest;
use crate::error::AppError;
use std::collections::HashMap;
use url::Url;

/// Parse a codexswift:// URL into a CodexSwiftDeepLinkRequest
pub fn parse_codexswift_url(url_str: &str) -> Result<CodexSwiftDeepLinkRequest, AppError> {
    let url = Url::parse(url_str)
        .map_err(|e| AppError::InvalidInput(format!("Invalid codexswift URL: {e}")))?;

    if url.scheme() != "codexswift" {
        return Err(AppError::InvalidInput(format!(
            "Invalid scheme: expected 'codexswift', got '{}'",
            url.scheme()
        )));
    }

    let version = url
        .host_str()
        .ok_or_else(|| AppError::InvalidInput("Missing version in URL host".to_string()))?;

    if version != "v1" {
        return Err(AppError::InvalidInput(format!(
            "Unsupported protocol version: {version}"
        )));
    }

    let action = url.path().trim_start_matches('/').to_string();
    if action.is_empty() {
        return Err(AppError::InvalidInput(
            "Missing action in URL path".to_string(),
        ));
    }

    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

    match action.as_str() {
        "login" => Ok(CodexSwiftDeepLinkRequest {
            action,
            base_url: params.get("baseUrl").cloned(),
            api_key: params.get("apiKey").cloned(),
            group_id: None,
        }),
        "import-group" => Ok(CodexSwiftDeepLinkRequest {
            action,
            base_url: None,
            api_key: None,
            group_id: params.get("groupId").cloned(),
        }),
        other => Err(AppError::InvalidInput(format!(
            "Unsupported codexswift action: {other}"
        ))),
    }
}
