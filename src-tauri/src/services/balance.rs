//! 供应商余额查询服务
//!
//! 支持 DeepSeek、StepFun、SiliconFlow、OpenRouter、Novita AI 的账户余额查询。
//! 返回 UsageResult 格式，与现有用量系统无缝对接。

use crate::provider::{UsageData, UsageResult};
use std::time::Duration;

// ── 供应商检测 ──────────────────────────────────────────────

enum BalanceProvider {
    NewApi,
    DeepSeek,
    StepFun,
    SiliconFlow,
    SiliconFlowEn,
    OpenRouter,
    NovitaAI,
}

fn detect_provider(base_url: &str) -> Option<BalanceProvider> {
    let url = base_url.to_lowercase();
    if url.contains("bistrocode.online") || url.contains("new-api") || url.contains("newapi") {
        Some(BalanceProvider::NewApi)
    } else if url.contains("api.deepseek.com") {
        Some(BalanceProvider::DeepSeek)
    } else if url.contains("api.stepfun.ai") || url.contains("api.stepfun.com") {
        Some(BalanceProvider::StepFun)
    } else if url.contains("api.siliconflow.cn") {
        Some(BalanceProvider::SiliconFlow)
    } else if url.contains("api.siliconflow.com") {
        Some(BalanceProvider::SiliconFlowEn)
    } else if url.contains("openrouter.ai") {
        Some(BalanceProvider::OpenRouter)
    } else if url.contains("api.novita.ai") {
        Some(BalanceProvider::NovitaAI)
    } else {
        None
    }
}

fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn normalize_new_api_site_url(base_url: &str) -> String {
    let base_url = normalize_base_url(base_url);
    for suffix in ["/compatible-mode/v1", "/openai/v1", "/api/v1", "/v1"] {
        if let Some(stripped) = base_url.strip_suffix(suffix) {
            return stripped.trim_end_matches('/').to_string();
        }
    }
    base_url
}

fn quota_to_usd(quota: Option<f64>) -> Option<f64> {
    quota.map(|value| value / 500_000.0)
}

fn make_error(msg: String) -> UsageResult {
    UsageResult {
        success: false,
        data: None,
        error: Some(msg),
    }
}

fn make_auth_error(status: reqwest::StatusCode) -> UsageResult {
    UsageResult {
        success: false,
        data: Some(vec![UsageData {
            plan_name: None,
            remaining: None,
            total: None,
            used: None,
            unit: None,
            is_valid: Some(false),
            invalid_message: Some(format!("Authentication failed (HTTP {status})")),
            extra: None,
        }]),
        error: Some(format!("Authentication failed (HTTP {status})")),
    }
}

// ── New API / BistroCode ────────────────────────────────────
// GET https://example.com/api/usage/token/
// Response: { code: true, data: { name, total_granted, total_used, total_available, unlimited_quota, expires_at } }

async fn query_new_api(base_url: &str, api_key: &str) -> UsageResult {
    let client = crate::proxy::http_client::get();
    let base_url = normalize_new_api_site_url(base_url);
    if base_url.is_empty() {
        return make_error("Base URL is empty".to_string());
    }

    let url = format!("{base_url}/api/usage/token/");
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    new_api_usage_from_value(&body)
}

fn new_api_usage_from_value(body: &serde_json::Value) -> UsageResult {
    let success = body
        .get("code")
        .and_then(|v| v.as_bool())
        .or_else(|| body.get("success").and_then(|v| v.as_bool()))
        .unwrap_or(true);
    if !success {
        let message = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("New API usage query failed");
        return make_error(message.to_string());
    }

    let data = body.get("data").unwrap_or(&body);
    let unlimited = data
        .get("unlimited_quota")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let name = data
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("BistroCode");
    let expires_at = data.get("expires_at").and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
    });
    let extra = expires_at.and_then(|ts| {
        if ts > 0 {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| format!("Expires {}", dt.format("%Y-%m-%d")))
        } else {
            None
        }
    });

    let remaining = quota_to_usd(parse_f64_field(data, "total_available"));
    let used = quota_to_usd(parse_f64_field(data, "total_used"));
    let total = quota_to_usd(parse_f64_field(data, "total_granted"));

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some(name.to_string()),
            remaining: if unlimited { Some(-1.0) } else { remaining },
            total: if unlimited { Some(-1.0) } else { total },
            used,
            unit: Some("USD".to_string()),
            is_valid: Some(unlimited || remaining.unwrap_or(0.0) > 0.0),
            invalid_message: if !unlimited && remaining.unwrap_or(0.0) <= 0.0 {
                Some("No credits remaining".to_string())
            } else {
                None
            },
            extra,
        }]),
        error: None,
    }
}

// ── DeepSeek ────────────────────────────────────────────────
// GET https://api.deepseek.com/user/balance
// Response: { balance_infos: [{ currency, total_balance, granted_balance, topped_up_balance }], is_available }

async fn query_deepseek(api_key: &str) -> UsageResult {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.deepseek.com/user/balance")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    let is_available = body
        .get("is_available")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let mut data = Vec::new();

    if let Some(infos) = body.get("balance_infos").and_then(|v| v.as_array()) {
        for info in infos {
            let currency = info
                .get("currency")
                .and_then(|v| v.as_str())
                .unwrap_or("CNY");
            let total = parse_f64_field(info, "total_balance");

            data.push(UsageData {
                plan_name: Some(currency.to_string()),
                remaining: total,
                total: None,
                used: None,
                unit: Some(currency.to_string()),
                is_valid: Some(is_available),
                invalid_message: if !is_available {
                    Some("Insufficient balance".to_string())
                } else {
                    None
                },
                extra: None,
            });
        }
    }

    UsageResult {
        success: true,
        data: if data.is_empty() { None } else { Some(data) },
        error: None,
    }
}

// ── StepFun ─────────────────────────────────────────────────
// GET https://api.stepfun.com/v1/accounts
// Response: { object, type, balance, total_cash_balance, total_voucher_balance }

async fn query_stepfun(api_key: &str) -> UsageResult {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.stepfun.com/v1/accounts")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    let balance = parse_f64_field(&body, "balance").unwrap_or(0.0);

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some("StepFun".to_string()),
            remaining: Some(balance),
            total: None,
            used: None,
            unit: Some("CNY".to_string()),
            is_valid: Some(true),
            invalid_message: None,
            extra: None,
        }]),
        error: None,
    }
}

// ── SiliconFlow ─────────────────────────────────────────────
// GET https://api.siliconflow.cn/v1/user/info (or .com for EN)
// Response: { code, data: { balance, chargeBalance, totalBalance, status } }

async fn query_siliconflow(api_key: &str, is_cn: bool) -> UsageResult {
    let client = crate::proxy::http_client::get();

    let domain = if is_cn {
        "api.siliconflow.cn"
    } else {
        "api.siliconflow.com"
    };
    let url = format!("https://{domain}/v1/user/info");

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    let data = match body.get("data") {
        Some(d) => d,
        None => return make_error("Missing 'data' field in response".to_string()),
    };

    let total_balance = parse_f64_field(data, "totalBalance").unwrap_or(0.0);

    let unit = if is_cn { "CNY" } else { "USD" };
    let plan_name = if is_cn {
        "SiliconFlow"
    } else {
        "SiliconFlow (EN)"
    };

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some(plan_name.to_string()),
            remaining: Some(total_balance),
            total: None,
            used: None,
            unit: Some(unit.to_string()),
            is_valid: Some(true),
            invalid_message: None,
            extra: None,
        }]),
        error: None,
    }
}

// ── OpenRouter ──────────────────────────────────────────────
// GET https://openrouter.ai/api/v1/credits
// Response: { data: { total_credits, total_usage } }

async fn query_openrouter(api_key: &str) -> UsageResult {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://openrouter.ai/api/v1/credits")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    let data = body.get("data").unwrap_or(&body);
    let total_credits = parse_f64_field(data, "total_credits").unwrap_or(0.0);
    let total_usage = parse_f64_field(data, "total_usage").unwrap_or(0.0);
    let remaining = total_credits - total_usage;

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some("OpenRouter".to_string()),
            remaining: Some(remaining),
            total: Some(total_credits),
            used: Some(total_usage),
            unit: Some("USD".to_string()),
            is_valid: Some(remaining > 0.0),
            invalid_message: if remaining <= 0.0 {
                Some("No credits remaining".to_string())
            } else {
                None
            },
            extra: None,
        }]),
        error: None,
    }
}

// ── Novita AI ───────────────────────────────────────────────
// GET https://api.novita.ai/v3/user/balance
// Response: { availableBalance, cashBalance, creditLimit, outstandingInvoices }
// 金额单位：0.0001 USD

async fn query_novita(api_key: &str) -> UsageResult {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.novita.ai/v3/user/balance")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return make_error(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return make_auth_error(status);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return make_error(format!("API error (HTTP {status}): {body}"));
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return make_error(format!("Failed to parse response: {e}")),
    };

    // Novita 金额单位为 0.0001 USD，需除以 10000 转为 USD
    let available = parse_f64_field(&body, "availableBalance").unwrap_or(0.0) / 10000.0;

    UsageResult {
        success: true,
        data: Some(vec![UsageData {
            plan_name: Some("Novita AI".to_string()),
            remaining: Some(available),
            total: None,
            used: None,
            unit: Some("USD".to_string()),
            is_valid: Some(available > 0.0),
            invalid_message: if available <= 0.0 {
                Some("No balance remaining".to_string())
            } else {
                None
            },
            extra: None,
        }]),
        error: None,
    }
}

// ── 工具函数 ────────────────────────────────────────────────

/// 解析 JSON 字段为 f64，兼容数字和字符串格式
fn parse_f64_field(obj: &serde_json::Value, field: &str) -> Option<f64> {
    obj.get(field).and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

// ── 公开入口 ────────────────────────────────────────────────

pub async fn get_balance(base_url: &str, api_key: &str) -> Result<UsageResult, String> {
    if api_key.trim().is_empty() {
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some("API key is empty".to_string()),
        });
    }

    let provider = match detect_provider(base_url) {
        Some(p) => p,
        None => {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: Some("Unknown balance provider".to_string()),
            })
        }
    };

    let result = match provider {
        BalanceProvider::NewApi => query_new_api(base_url, api_key).await,
        BalanceProvider::DeepSeek => query_deepseek(api_key).await,
        BalanceProvider::StepFun => query_stepfun(api_key).await,
        BalanceProvider::SiliconFlow => query_siliconflow(api_key, true).await,
        BalanceProvider::SiliconFlowEn => query_siliconflow(api_key, false).await,
        BalanceProvider::OpenRouter => query_openrouter(api_key).await,
        BalanceProvider::NovitaAI => query_novita(api_key).await,
    };

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        detect_provider, new_api_usage_from_value, normalize_new_api_site_url, BalanceProvider,
    };
    use serde_json::json;

    #[test]
    fn detects_bistrocode_as_new_api_provider() {
        assert!(matches!(
            detect_provider("https://bistrocode.online/v1"),
            Some(BalanceProvider::NewApi)
        ));
    }

    #[test]
    fn strips_openai_compat_suffix_for_new_api_usage_endpoint() {
        assert_eq!(
            normalize_new_api_site_url("https://bistrocode.online/v1"),
            "https://bistrocode.online"
        );
        assert_eq!(
            normalize_new_api_site_url("https://example.com/api/v1/"),
            "https://example.com"
        );
        assert_eq!(
            normalize_new_api_site_url("https://example.com/compatible-mode/v1"),
            "https://example.com"
        );
    }

    #[test]
    fn parses_new_api_quota_response_as_usd_usage() {
        let result = new_api_usage_from_value(&json!({
            "code": true,
            "data": {
                "name": "BistroCode Pro",
                "total_granted": 1_500_000,
                "total_used": "500000",
                "total_available": 1_000_000,
                "unlimited_quota": false,
                "expires_at": 1893456000
            }
        }));

        assert!(result.success);
        let row = result.data.as_ref().unwrap().first().unwrap();
        assert_eq!(row.plan_name.as_deref(), Some("BistroCode Pro"));
        assert_eq!(row.total, Some(3.0));
        assert_eq!(row.used, Some(1.0));
        assert_eq!(row.remaining, Some(2.0));
        assert_eq!(row.unit.as_deref(), Some("USD"));
        assert_eq!(row.is_valid, Some(true));
        assert_eq!(row.extra.as_deref(), Some("Expires 2030-01-01"));
    }

    #[test]
    fn parses_new_api_unlimited_quota() {
        let result = new_api_usage_from_value(&json!({
            "success": true,
            "data": {
                "name": "Unlimited",
                "total_used": 250000,
                "unlimited_quota": true
            }
        }));

        let row = result.data.as_ref().unwrap().first().unwrap();
        assert_eq!(row.remaining, Some(-1.0));
        assert_eq!(row.total, Some(-1.0));
        assert_eq!(row.used, Some(0.5));
        assert_eq!(row.is_valid, Some(true));
        assert_eq!(row.invalid_message, None);
    }
}
