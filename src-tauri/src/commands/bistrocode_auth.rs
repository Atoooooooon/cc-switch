use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;
const BISTROCODE_AUTH_BASE_URL: &str = "https://bistrocode.online";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodeAuthSessionRequest {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodeAuthSessionResponse {
    pub success: bool,
    pub message: Option<String>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodeAuthExchangeResponse {
    pub success: bool,
    pub message: Option<String>,
    pub data: Option<BistroCodeAuthUser>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodeAuthUser {
    pub id: i64,
    pub username: String,
    #[serde(alias = "display_name")]
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub group: Option<String>,
    pub quota: i64,
    #[serde(alias = "used_quota")]
    #[serde(alias = "usedQuota")]
    pub used_quota: i64,
    #[serde(alias = "request_count")]
    #[serde(alias = "requestCount")]
    pub request_count: i64,
    #[serde(alias = "access_token")]
    #[serde(alias = "accessToken")]
    pub access_token: String,
}

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct BistroCodeQuotaDataItem {
    pub id: Option<i64>,
    pub user_id: Option<i64>,
    pub username: Option<String>,
    pub model_name: Option<String>,
    pub created_at: i64,
    pub token_used: Option<i64>,
    pub count: Option<i64>,
    pub quota: Option<i64>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct BistroCodeQuotaDataResponse {
    pub success: bool,
    pub message: Option<String>,
    pub data: Option<Vec<BistroCodeQuotaDataItem>>,
}

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodeDesktopTokenConfig {
    pub id: i64,
    pub name: String,
    pub key: String,
    pub group: String,
    pub purpose: String,
    #[serde(alias = "model_match")]
    pub model_match: String,
    #[serde(alias = "base_url")]
    pub base_url: String,
    #[serde(alias = "created_time")]
    pub created_time: i64,
}

#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodeDesktopTokensData {
    pub configs: Vec<BistroCodeDesktopTokenConfig>,
    pub groups: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodeDesktopTokensResponse {
    pub success: bool,
    pub message: Option<String>,
    pub data: Option<BistroCodeDesktopTokensData>,
}

fn bistrocode_authorization_header(token: &str) -> String {
    if token.to_lowercase().starts_with("bearer ") {
        token.to_string()
    } else {
        format!("Bearer {token}")
    }
}

#[tauri::command]
pub async fn exchange_bistrocode_auth_code(
    code: String,
    state: String,
) -> Result<BistroCodeAuthExchangeResponse, String> {
    let client = crate::proxy::http_client::get();
    let response = client
        .post(format!("{BISTROCODE_AUTH_BASE_URL}/api/desktop/exchange"))
        .timeout(Duration::from_secs(20))
        .json(&serde_json::json!({
            "code": code,
            "state": state
        }))
        .send()
        .await
        .map_err(|e| format!("Failed to exchange BistroCode auth code: {e}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|json| {
                json.get("message")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string)
            })
            .filter(|message| !message.trim().is_empty())
            .unwrap_or(body);
        return Err(format!(
            "Failed to exchange BistroCode auth code (HTTP {status}): {message}"
        ));
    }

    let result = serde_json::from_str::<BistroCodeAuthExchangeResponse>(&body)
        .map_err(|e| format!("Failed to parse BistroCode auth response: {e}"))?;
    if !result.success {
        return Err(result
            .message
            .clone()
            .unwrap_or_else(|| "BistroCode authorization failed".to_string()));
    }
    Ok(result)
}

#[tauri::command]
pub async fn get_bistrocode_dashboard_quota(
    #[allow(non_snake_case)] accessToken: String,
    #[allow(non_snake_case)] userId: String,
    #[allow(non_snake_case)] startTimestamp: i64,
    #[allow(non_snake_case)] endTimestamp: i64,
) -> Result<Vec<BistroCodeQuotaDataItem>, String> {
    let token = accessToken.trim();
    let user_id = userId.trim();
    if token.is_empty() || user_id.is_empty() {
        return Err("BistroCode account is not connected".to_string());
    }

    let client = crate::proxy::http_client::get();
    let response = client
        .get(format!("{BISTROCODE_AUTH_BASE_URL}/api/data/self"))
        .timeout(Duration::from_secs(20))
        .header("Accept", "application/json")
        .header("Authorization", bistrocode_authorization_header(token))
        .header("New-Api-User", user_id)
        .query(&[
            ("start_timestamp", startTimestamp.to_string()),
            ("end_timestamp", endTimestamp.to_string()),
            ("default_time", "hour".to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("查询 BistroCode 云端看板失败: {e}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "查询 BistroCode 云端看板失败 (HTTP {status}): {body}"
        ));
    }

    let result = serde_json::from_str::<BistroCodeQuotaDataResponse>(&body)
        .map_err(|e| format!("解析 BistroCode 云端看板失败: {e}"))?;
    if !result.success {
        return Err(result
            .message
            .unwrap_or_else(|| "BistroCode 云端看板返回失败".to_string()));
    }

    Ok(result.data.unwrap_or_default())
}

#[tauri::command]
pub async fn ensure_bistrocode_default_tokens(
    #[allow(non_snake_case)] accessToken: String,
    #[allow(non_snake_case)] userId: String,
) -> Result<Vec<BistroCodeDesktopTokenConfig>, String> {
    let token = accessToken.trim();
    let user_id = userId.trim();
    if token.is_empty() || user_id.is_empty() {
        return Err("BistroCode account is not connected".to_string());
    }

    let client = crate::proxy::http_client::get();
    let response = client
        .post(format!(
            "{BISTROCODE_AUTH_BASE_URL}/api/desktop/default-tokens"
        ))
        .timeout(Duration::from_secs(20))
        .header("Accept", "application/json")
        .header("Authorization", bistrocode_authorization_header(token))
        .header("New-Api-User", user_id)
        .send()
        .await
        .map_err(|e| format!("创建 BistroCode 默认 API Key 失败: {e}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "创建 BistroCode 默认 API Key 失败 (HTTP {status}): {body}"
        ));
    }

    let result = serde_json::from_str::<BistroCodeDesktopTokensResponse>(&body)
        .map_err(|e| format!("解析 BistroCode 默认 API Key 失败: {e}"))?;
    if !result.success {
        return Err(result
            .message
            .unwrap_or_else(|| "BistroCode 默认 API Key 返回失败".to_string()));
    }

    Ok(result.data.map(|data| data.configs).unwrap_or_default())
}
