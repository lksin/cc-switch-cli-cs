use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CodexSwiftAccount {
    pub user_id: serde_json::Value,
    pub username: String,
    pub role: String,
    pub key_id: serde_json::Value,
    pub key_name: Option<String>,
    pub balance: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aff_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CodexSwiftGroup {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub group_type: String,
    pub status: String,
    #[serde(default)]
    pub active_users: i64,
    #[serde(
        default = "default_multiplier",
        alias = "cost_multiplier",
        alias = "costMultiplier",
        alias = "price_multiplier",
        alias = "priceMultiplier",
        deserialize_with = "deserialize_multiplier"
    )]
    pub multiplier: f64,
}

fn default_multiplier() -> f64 {
    1.0
}

fn deserialize_multiplier<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(number) => number
            .as_f64()
            .ok_or_else(|| serde::de::Error::custom("invalid multiplier number")),
        serde_json::Value::String(value) => value
            .trim()
            .parse::<f64>()
            .map_err(serde::de::Error::custom),
        serde_json::Value::Null => Ok(default_multiplier()),
        other => Err(serde::de::Error::custom(format!(
            "invalid multiplier value: {other}"
        ))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CodexSwiftUsage {
    pub balance: f64,
    #[serde(default)]
    pub transactions: Vec<serde_json::Value>,
    #[serde(default)]
    pub active_groups: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CodexSwiftSessionResponse {
    pub session_id: serde_json::Value,
    pub config_b64: String,
    pub group: CodexSwiftGroup,
    pub balance: f64,
    pub started_at: String,
}

/// Fernet 解密后得到的明文 Provider 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptedConfig {
    #[serde(rename = "type")]
    pub provider_type: String,
    pub url: String,
    pub key: String,
    pub sid: i64,
}

/// 存入 SQLite settings 表的活跃 Session 状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSwiftActiveSession {
    pub session_id: String,
    pub group_id: String,
    pub group_name: String,
    pub balance: f64,
    pub started_at: String,
}

/// 返回给前端的配置（不含 api_key）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSwiftConfig {
    pub base_url: String,
}
