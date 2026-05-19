use crate::provider::UsageResult;
use serde::Serialize;
use std::time::Duration;

const BISTROCODE_QUOTA_PER_USD: f64 = 500_000.0;

#[tauri::command]
pub async fn get_balance(base_url: String, api_key: String) -> Result<UsageResult, String> {
    crate::services::balance::get_balance(&base_url, &api_key).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodeAccount {
    pub id: Option<i64>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub group: Option<String>,
    pub quota: i64,
    pub used_quota: i64,
    pub request_count: Option<i64>,
    pub quota_per_usd: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodeAccountResponse {
    pub logged_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<BistroCodeAccount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodePricingModel {
    pub model_name: String,
    pub quota_type: i64,
    pub model_ratio: f64,
    pub model_price: f64,
    pub completion_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_cache_ratio: Option<f64>,
    pub enable_groups: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodePricingResponse {
    pub success: bool,
    pub models: Vec<BistroCodePricingModel>,
    pub group_ratio: std::collections::HashMap<String, f64>,
    pub default_group_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub(crate) async fn fetch_bistrocode_pricing() -> Result<BistroCodePricingResponse, String> {
    let client = crate::proxy::http_client::get();
    let response = client
        .get("https://bistrocode.online/api/pricing")
        .timeout(Duration::from_secs(20))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Failed to query BistroCode pricing: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "BistroCode pricing query failed (HTTP {status}): {body}"
        ));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse BistroCode pricing response: {e}"))?;

    let success = body
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !success {
        let message = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("BistroCode pricing query failed")
            .to_string();
        return Ok(BistroCodePricingResponse {
            success: false,
            models: Vec::new(),
            group_ratio: std::collections::HashMap::new(),
            default_group_ratio: 0.2,
            message: Some(message),
        });
    }

    let default_group_ratio = body
        .get("group_ratio")
        .and_then(|v| v.get("default"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.2);

    let group_ratio = body
        .get("group_ratio")
        .and_then(|v| v.as_object())
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| value.as_f64().map(|ratio| (key.clone(), ratio)))
                .collect::<std::collections::HashMap<String, f64>>()
        })
        .unwrap_or_default();

    let models = body
        .get("data")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| BistroCodePricingModel {
                    model_name: item
                        .get("model_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    quota_type: item.get("quota_type").and_then(|v| v.as_i64()).unwrap_or(0),
                    model_ratio: item
                        .get("model_ratio")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    model_price: item
                        .get("model_price")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    completion_ratio: item
                        .get("completion_ratio")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    cache_ratio: item.get("cache_ratio").and_then(|v| v.as_f64()),
                    create_cache_ratio: item.get("create_cache_ratio").and_then(|v| v.as_f64()),
                    enable_groups: item
                        .get("enable_groups")
                        .and_then(|v| v.as_array())
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(BistroCodePricingResponse {
        success: true,
        models,
        group_ratio,
        default_group_ratio,
        message: None,
    })
}

#[tauri::command]
pub async fn get_bistrocode_account(
    #[allow(non_snake_case)] accessToken: Option<String>,
    #[allow(non_snake_case)] userId: Option<String>,
) -> Result<BistroCodeAccountResponse, String> {
    let client = crate::proxy::http_client::get();
    let mut request = client
        .get("https://bistrocode.online/api/user/self")
        .timeout(Duration::from_secs(20))
        .header("Accept", "application/json");

    if let Some(token) = accessToken
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let authorization = if token.to_lowercase().starts_with("bearer ") {
            token.to_string()
        } else {
            format!("Bearer {token}")
        };
        request = request.header("Authorization", authorization);
    }

    if let Some(user_id) = userId.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        request = request.header("New-Api-User", user_id);
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("Failed to query BistroCode account: {e}"))?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        let body = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|json| {
                json.get("message")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| {
                if body.trim().is_empty() {
                    "BistroCode account is not logged in".to_string()
                } else {
                    body
                }
            });
        return Ok(BistroCodeAccountResponse {
            logged_in: false,
            account: None,
            message: Some(message),
        });
    }

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "BistroCode account query failed (HTTP {status}): {body}"
        ));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse BistroCode account response: {e}"))?;
    let success = body
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !success {
        let message = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("BistroCode account is not logged in")
            .to_string();
        return Ok(BistroCodeAccountResponse {
            logged_in: false,
            account: None,
            message: Some(message),
        });
    }

    let data = body.get("data").unwrap_or(&body);
    Ok(BistroCodeAccountResponse {
        logged_in: true,
        account: Some(BistroCodeAccount {
            id: parse_i64(data.get("id")),
            username: parse_string(data.get("username")),
            display_name: parse_string(data.get("display_name")),
            email: parse_string(data.get("email")),
            group: parse_string(data.get("group")),
            quota: parse_i64(data.get("quota")).unwrap_or(0),
            used_quota: parse_i64(data.get("used_quota")).unwrap_or(0),
            request_count: parse_i64(data.get("request_count")),
            quota_per_usd: BISTROCODE_QUOTA_PER_USD,
        }),
        message: None,
    })
}

#[tauri::command]
pub async fn get_bistrocode_pricing() -> Result<BistroCodePricingResponse, String> {
    fetch_bistrocode_pricing().await
}

fn parse_i64(value: Option<&serde_json::Value>) -> Option<i64> {
    value.and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_u64().and_then(|n| i64::try_from(n).ok()))
            .or_else(|| v.as_f64().map(|n| n as i64))
            .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
    })
}

fn parse_string(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}
