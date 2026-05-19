//! 使用统计相关命令

use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::usage_stats::*;
use crate::store::AppState;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;
use tauri::State;

const BISTROCODE_QUOTA_PER_USD: i64 = 500_000;

/// 获取使用量汇总
#[tauri::command]
pub fn get_usage_summary(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
) -> Result<UsageSummary, AppError> {
    state
        .db
        .get_usage_summary(start_date, end_date, app_type.as_deref())
}

/// 获取按 app_type 拆分的使用量汇总
#[tauri::command]
pub fn get_usage_summary_by_app(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<UsageSummaryByApp>, AppError> {
    state.db.get_usage_summary_by_app(start_date, end_date)
}

/// 获取每日趋势
#[tauri::command]
pub fn get_usage_trends(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
) -> Result<Vec<DailyStats>, AppError> {
    state
        .db
        .get_daily_trends(start_date, end_date, app_type.as_deref())
}

/// 获取 Provider 统计
#[tauri::command]
pub fn get_provider_stats(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
) -> Result<Vec<ProviderStats>, AppError> {
    state
        .db
        .get_provider_stats(start_date, end_date, app_type.as_deref())
}

/// 获取模型统计
#[tauri::command]
pub fn get_model_stats(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
) -> Result<Vec<ModelStats>, AppError> {
    state
        .db
        .get_model_stats(start_date, end_date, app_type.as_deref())
}

/// 获取请求日志列表
#[tauri::command]
pub fn get_request_logs(
    state: State<'_, AppState>,
    filters: LogFilters,
    page: u32,
    page_size: u32,
) -> Result<PaginatedLogs, AppError> {
    state.db.get_request_logs(&filters, page, page_size)
}

/// 获取单个请求详情
#[tauri::command]
pub fn get_request_detail(
    state: State<'_, AppState>,
    request_id: String,
) -> Result<Option<RequestLogDetail>, AppError> {
    state.db.get_request_detail(&request_id)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BistroCodeUsageEstimate {
    pub official_cost_usd: String,
    pub bistrocode_cost_usd: String,
    pub estimated_savings_usd: String,
    pub bistrocode_quota_used: u64,
    pub priced_requests: u64,
    pub unpriced_requests: u64,
    pub total_requests: u64,
    pub official_total_tokens: u64,
    pub bistrocode_total_tokens: u64,
}

#[tauri::command]
pub async fn get_bistrocode_usage_estimate(
    state: State<'_, AppState>,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<BistroCodeUsageEstimate, AppError> {
    let db = state.db.clone();
    let pricing = crate::commands::balance::fetch_bistrocode_pricing()
        .await
        .map_err(AppError::Config)?;
    let summary = db.get_usage_summary(start_date, end_date, None)?;
    let logs = db.get_request_logs(
        &LogFilters {
            start_date,
            end_date,
            ..Default::default()
        },
        0,
        10_000,
    )?;

    let default_group_ratio =
        Decimal::from_str(&pricing.default_group_ratio.to_string()).unwrap_or(Decimal::new(2, 1));
    let pricing_models = pricing.models;

    let mut bistrocode_total = Decimal::ZERO;
    let mut bistrocode_quota_used: u64 = 0;
    let mut official_total = Decimal::ZERO;
    let mut priced_requests: u64 = 0;
    let mut unpriced_requests: u64 = 0;

    for log in logs.data {
        let model_pricing = {
            let conn = crate::database::lock_conn!(db.conn);
            crate::services::usage_stats::find_model_pricing(&conn, &log.model)
        };

        let Some(model_pricing) = model_pricing else {
            unpriced_requests += 1;
            continue;
        };

        let usage = TokenUsage {
            input_tokens: log.input_tokens,
            output_tokens: log.output_tokens,
            cache_read_tokens: log.cache_read_tokens,
            cache_creation_tokens: log.cache_creation_tokens,
            model: None,
            message_id: None,
        };
        let official =
            CostCalculator::calculate_for_app(&log.app_type, &usage, &model_pricing, Decimal::ONE)
                .total_cost;
        official_total += official;

        let Some(bistro_model) = find_bistrocode_pricing(&pricing_models, &log.model) else {
            unpriced_requests += 1;
            continue;
        };

        let quota =
            estimate_bistrocode_quota(&log.app_type, &usage, bistro_model, default_group_ratio);
        bistrocode_quota_used = bistrocode_quota_used.saturating_add(quota);
        bistrocode_total += Decimal::from(quota) / Decimal::from(BISTROCODE_QUOTA_PER_USD);
        priced_requests += 1;
    }

    let savings = if official_total > bistrocode_total {
        official_total - bistrocode_total
    } else {
        Decimal::ZERO
    };

    Ok(BistroCodeUsageEstimate {
        official_cost_usd: format!("{:.6}", official_total),
        bistrocode_cost_usd: format!("{:.6}", bistrocode_total),
        estimated_savings_usd: format!("{:.6}", savings),
        bistrocode_quota_used,
        priced_requests,
        unpriced_requests,
        total_requests: summary.total_requests,
        official_total_tokens: summary.real_total_tokens,
        bistrocode_total_tokens: summary.real_total_tokens,
    })
}

fn find_bistrocode_pricing<'a>(
    models: &'a [crate::commands::balance::BistroCodePricingModel],
    model: &str,
) -> Option<&'a crate::commands::balance::BistroCodePricingModel> {
    let normalized = normalize_model_id(model);
    models
        .iter()
        .find(|item| normalize_model_id(&item.model_name) == normalized)
        .or_else(|| {
            models.iter().find(|item| {
                let item_id = normalize_model_id(&item.model_name);
                !item_id.is_empty() && normalized.starts_with(&item_id)
            })
        })
}

fn normalize_model_id(model: &str) -> String {
    model
        .trim()
        .trim_matches('"')
        .to_ascii_lowercase()
        .replace("openai/", "")
        .replace("anthropic/", "")
        .replace("google/", "")
}

fn decimal_from_f64(value: f64, fallback: Decimal) -> Decimal {
    Decimal::from_str(&value.to_string()).unwrap_or(fallback)
}

fn estimate_bistrocode_quota(
    app_type: &str,
    usage: &TokenUsage,
    pricing: &crate::commands::balance::BistroCodePricingModel,
    group_ratio: Decimal,
) -> u64 {
    let model_ratio = decimal_from_f64(pricing.model_ratio, Decimal::ZERO);
    let completion_ratio = decimal_from_f64(pricing.completion_ratio, Decimal::ONE);
    let cache_ratio = decimal_from_f64(pricing.cache_ratio.unwrap_or(1.0), Decimal::ONE);
    let create_cache_ratio =
        decimal_from_f64(pricing.create_cache_ratio.unwrap_or(1.0), Decimal::ONE);

    if pricing.quota_type == 1 {
        let model_price = decimal_from_f64(pricing.model_price, Decimal::ZERO);
        let quota = model_price * Decimal::from(BISTROCODE_QUOTA_PER_USD) * group_ratio;
        return quota.round().to_u64().unwrap_or(0);
    }

    let mut input_tokens = Decimal::from(usage.input_tokens);
    let cache_read_tokens = Decimal::from(usage.cache_read_tokens);
    let cache_creation_tokens = Decimal::from(usage.cache_creation_tokens);

    if matches!(app_type, "codex" | "gemini") {
        input_tokens -= cache_read_tokens;
        input_tokens -= cache_creation_tokens;
        if input_tokens < Decimal::ZERO {
            input_tokens = Decimal::ZERO;
        }
    }

    let weighted_tokens = input_tokens
        + Decimal::from(usage.output_tokens) * completion_ratio
        + cache_read_tokens * cache_ratio
        + cache_creation_tokens * create_cache_ratio;
    let quota = weighted_tokens * model_ratio * group_ratio;

    if quota > Decimal::ZERO && quota < Decimal::ONE {
        return 1;
    }
    quota.round().to_u64().unwrap_or(0)
}

/// 获取模型定价列表
#[tauri::command]
pub fn get_model_pricing(state: State<'_, AppState>) -> Result<Vec<ModelPricingInfo>, AppError> {
    log::info!("获取模型定价列表");
    state.db.ensure_model_pricing_seeded()?;

    let db = state.db.clone();
    let conn = crate::database::lock_conn!(db.conn);

    // 检查表是否存在
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='model_pricing'",
            [],
            |row| row.get::<_, i64>(0).map(|count| count > 0),
        )
        .unwrap_or(false);

    if !table_exists {
        log::error!("model_pricing 表不存在,可能需要重启应用以触发数据库迁移");
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing
         ORDER BY display_name",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(ModelPricingInfo {
            model_id: row.get(0)?,
            display_name: row.get(1)?,
            input_cost_per_million: row.get(2)?,
            output_cost_per_million: row.get(3)?,
            cache_read_cost_per_million: row.get(4)?,
            cache_creation_cost_per_million: row.get(5)?,
        })
    })?;

    let mut pricing = Vec::new();
    for row in rows {
        pricing.push(row?);
    }

    log::info!("成功获取 {} 条模型定价数据", pricing.len());
    Ok(pricing)
}

/// 更新模型定价
#[tauri::command]
pub fn update_model_pricing(
    state: State<'_, AppState>,
    model_id: String,
    display_name: String,
    input_cost: String,
    output_cost: String,
    cache_read_cost: String,
    cache_creation_cost: String,
) -> Result<(), AppError> {
    let db = state.db.clone();
    let model_id = model_id.trim().to_string();
    let display_name = display_name.trim().to_string();
    if model_id.is_empty() {
        return Err(AppError::localized(
            "usage.modelIdRequired",
            "模型 ID 不能为空",
            "Model ID is required",
        ));
    }
    if display_name.is_empty() {
        return Err(AppError::localized(
            "usage.displayNameRequired",
            "显示名称不能为空",
            "Display name is required",
        ));
    }

    for (label, value) in [
        ("input_cost", &input_cost),
        ("output_cost", &output_cost),
        ("cache_read_cost", &cache_read_cost),
        ("cache_creation_cost", &cache_creation_cost),
    ] {
        let parsed = Decimal::from_str(value.trim()).map_err(|e| {
            AppError::localized(
                "usage.invalidPrice",
                format!("{label} 价格无效: {value} - {e}"),
                format!("{label} price is invalid: {value} - {e}"),
            )
        })?;
        if parsed < Decimal::ZERO {
            return Err(AppError::localized(
                "usage.invalidPrice",
                format!("{label} 价格必须为非负数: {value}"),
                format!("{label} price must be non-negative: {value}"),
            ));
        }
    }

    {
        let conn = crate::database::lock_conn!(db.conn);
        conn.execute(
            "INSERT OR REPLACE INTO model_pricing (
                model_id, display_name, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                model_id,
                display_name,
                input_cost.trim(),
                output_cost.trim(),
                cache_read_cost.trim(),
                cache_creation_cost.trim()
            ],
        )
        .map_err(|e| AppError::Database(format!("更新模型定价失败: {e}")))?;
    }

    if let Err(e) = db.backfill_missing_usage_costs_for_model(&model_id) {
        log::warn!("模型定价更新后回填历史用量成本失败 (model_id={model_id}): {e}");
    }

    Ok(())
}

/// 检查 Provider 使用限额
#[tauri::command]
pub fn check_provider_limits(
    state: State<'_, AppState>,
    provider_id: String,
    app_type: String,
) -> Result<crate::services::usage_stats::ProviderLimitStatus, AppError> {
    state.db.check_provider_limits(&provider_id, &app_type)
}

/// 删除模型定价
#[tauri::command]
pub fn delete_model_pricing(state: State<'_, AppState>, model_id: String) -> Result<(), AppError> {
    let db = state.db.clone();
    let conn = crate::database::lock_conn!(db.conn);

    conn.execute(
        "DELETE FROM model_pricing WHERE model_id = ?1",
        rusqlite::params![model_id],
    )
    .map_err(|e| AppError::Database(format!("删除模型定价失败: {e}")))?;

    log::info!("已删除模型定价: {model_id}");
    Ok(())
}

/// 手动触发会话日志同步
#[tauri::command]
pub fn sync_session_usage(
    state: State<'_, AppState>,
) -> Result<crate::services::session_usage::SessionSyncResult, AppError> {
    // 同步 Claude 会话日志
    let mut result = crate::services::session_usage::sync_claude_session_logs(&state.db)?;

    // 同步 Codex 使用数据
    match crate::services::session_usage_codex::sync_codex_usage(&state.db) {
        Ok(codex_result) => {
            result.imported += codex_result.imported;
            result.skipped += codex_result.skipped;
            result.files_scanned += codex_result.files_scanned;
            result.errors.extend(codex_result.errors);
        }
        Err(e) => {
            result.errors.push(format!("Codex 同步失败: {e}"));
        }
    }

    // 同步 Gemini 使用数据
    match crate::services::session_usage_gemini::sync_gemini_usage(&state.db) {
        Ok(gemini_result) => {
            result.imported += gemini_result.imported;
            result.skipped += gemini_result.skipped;
            result.files_scanned += gemini_result.files_scanned;
            result.errors.extend(gemini_result.errors);
        }
        Err(e) => {
            result.errors.push(format!("Gemini 同步失败: {e}"));
        }
    }

    Ok(result)
}

/// 获取数据来源分布
#[tauri::command]
pub fn get_usage_data_sources(
    state: State<'_, AppState>,
) -> Result<Vec<crate::services::session_usage::DataSourceSummary>, AppError> {
    crate::services::session_usage::get_data_source_breakdown(&state.db)
}

/// 模型定价信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingInfo {
    pub model_id: String,
    pub display_name: String,
    pub input_cost_per_million: String,
    pub output_cost_per_million: String,
    pub cache_read_cost_per_million: String,
    pub cache_creation_cost_per_million: String,
}
