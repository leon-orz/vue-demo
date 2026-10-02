//! MiMo 配额适配器（手录总额 + 本地推算，无网络请求）。
//! MiMo 无公开配额 API：套餐月度 Credit 总额（quota_total）与计费周期（quota_cycle）
//! 由用户在 platform_account 手录；消耗进度从 usage_event 汇总当前计费周期内
//! platform='mimo' 的 cost（mimocode 日志自带成本字段）推算。
//! 未配置总额属于"模块不可用"语义，返回 AdapterError::Unavailable 而非 panic。

use chrono::{Datelike, Local, SecondsFormat, TimeZone, Utc};
use sqlx::sqlite::SqlitePool;

use super::AdapterError;
use crate::models::{Platform, QuotaSnapshot};

/// 当前支持的计费周期取值（platform_account.quota_cycle）
const CYCLE_MONTHLY: &str = "monthly";

pub struct MimoAdapter;

impl MimoAdapter {
    /// 本地推算 MiMo 套餐配额快照。
    /// 数据只读自本模块负责的两张表：platform_account（手录总额/周期）、usage_event（周期内 cost 求和）。
    pub async fn snapshot(pool: &SqlitePool) -> Result<QuotaSnapshot, AdapterError> {
        // 取最早配置的 mimo 账户（个人自用场景实际只有一个）
        let account: Option<(Option<f64>, Option<String>, i64)> = sqlx::query_as(
            "SELECT quota_total, quota_cycle, enabled FROM platform_account
             WHERE platform = ? ORDER BY id LIMIT 1",
        )
        .bind(Platform::Mimo.as_str())
        .fetch_optional(pool)
        .await?;
        let (quota_total, quota_cycle, enabled) = account.ok_or_else(|| {
            AdapterError::Unavailable("尚未配置 MiMo 账户，请在设置中手录套餐".to_string())
        })?;
        if enabled == 0 {
            return Err(AdapterError::Unavailable("MiMo 账户已停用".to_string()));
        }
        let total = quota_total.filter(|t| *t > 0.0).ok_or_else(|| {
            AdapterError::Unavailable(
                "MiMo 套餐月度总额未配置，请在设置中手录 quota_total".to_string(),
            )
        })?;
        // 未填周期默认按月度处理
        let cycle = quota_cycle.as_deref().unwrap_or(CYCLE_MONTHLY);
        if cycle != CYCLE_MONTHLY {
            return Err(AdapterError::Unavailable(format!(
                "暂不支持的计费周期: {cycle}（当前仅支持 {CYCLE_MONTHLY}）"
            )));
        }

        let period_start_ms = current_month_start_ms();
        // 当前计费周期内的消耗 = 日志自带 cost 求和（cost 为 NULL 的事件不参与成本推算）
        let (used,): (f64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(cost), 0.0) FROM usage_event
             WHERE platform = ? AND ts >= ? AND cost IS NOT NULL",
        )
        .bind(Platform::Mimo.as_str())
        .bind(period_start_ms)
        .fetch_one(pool)
        .await?;

        // 超量时 remaining 为负、progress > 1，交由前端按"已超额"展示，不做截断
        let remaining = total - used;
        let progress = used / total;
        let period_start = Local
            .timestamp_millis_opt(period_start_ms)
            .single()
            .map(|t| t.format("%Y-%m-%d").to_string());
        Ok(QuotaSnapshot {
            platform: Platform::Mimo,
            total: Some(total),
            used: Some(used),
            remaining: Some(remaining),
            detail_json: Some(
                serde_json::json!({
                    "quota_cycle": CYCLE_MONTHLY,
                    "period_start": period_start,
                    "progress": progress,
                    "unit": "credit",
                })
                .to_string(),
            ),
            fetched_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        })
    }
}

/// 当前计费周期起点（月度周期 = 本地本月 1 号零点）的 Unix 毫秒；
/// 极端情况下回退为当前时刻（宁可少算，不错算上个周期）
fn current_month_start_ms() -> i64 {
    let today = Local::now().date_naive();
    today
        .with_day(1)
        .unwrap_or(today)
        .and_hms_opt(0, 0, 0)
        .and_then(|dt| Local.from_local_datetime(&dt).single())
        .map(|dt| dt.timestamp_millis())
        .unwrap_or_else(|| Utc::now().timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;

    async fn temp_pool() -> (tempfile::TempDir, SqlitePool) {
        let tmp = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(tmp.path().join("t.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();
        (tmp, pool)
    }

    async fn set_account(pool: &SqlitePool, quota_total: Option<f64>, cycle: Option<&str>) {
        sqlx::query(
            "INSERT INTO platform_account (platform, name, quota_total, quota_cycle, enabled)
             VALUES ('mimo', '默认', ?, ?, 1)",
        )
        .bind(quota_total)
        .bind(cycle)
        .execute(pool)
        .await
        .unwrap();
    }

    /// 造一条 mimo 用量事件（先补 log_file 行满足外键）
    async fn add_event(pool: &SqlitePool, ts: i64, cost: Option<f64>) {
        sqlx::query(
            "INSERT INTO log_file (platform, path, offset, size, mtime, last_scanned_at)
             VALUES ('mimo', 'mimocode.db', 0, 0, 0, '2026-01-01T00:00:00Z')
             ON CONFLICT(platform, path) DO NOTHING",
        )
        .execute(pool)
        .await
        .unwrap();
        let (file_id,): (i64,) = sqlx::query_as(
            "SELECT id FROM log_file WHERE platform = 'mimo' AND path = 'mimocode.db'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO usage_event
                 (platform, cli, log_file_id, ts, session_id, project, model,
                  input_other, output, cache_read, cache_creation, cost)
             VALUES ('mimo', 'mimocode', ?, ?, 's1', 'app', 'mimo-v1', 1, 1, 0, 0, ?)",
        )
        .bind(file_id)
        .bind(ts)
        .bind(cost)
        .execute(pool)
        .await
        .unwrap();
    }

    /// 本月 1 号零点（本地时区）的 Unix 毫秒，与实现共用同一边界
    fn month_start_ms() -> i64 {
        current_month_start_ms()
    }

    #[tokio::test]
    async fn unavailable_when_account_missing() {
        let (_tmp, pool) = temp_pool().await;
        let err = MimoAdapter::snapshot(&pool).await.unwrap_err();
        assert!(matches!(err, AdapterError::Unavailable(_)));
    }

    #[tokio::test]
    async fn unavailable_when_quota_total_not_configured() {
        let (_tmp, pool) = temp_pool().await;
        // 总额 NULL 与 0 都视为未配置
        set_account(&pool, None, Some("monthly")).await;
        let err = MimoAdapter::snapshot(&pool).await.unwrap_err();
        assert!(matches!(err, AdapterError::Unavailable(_)));

        sqlx::query("UPDATE platform_account SET quota_total = 0.0 WHERE platform = 'mimo'")
            .execute(&pool)
            .await
            .unwrap();
        let err = MimoAdapter::snapshot(&pool).await.unwrap_err();
        assert!(matches!(err, AdapterError::Unavailable(_)));
    }

    #[tokio::test]
    async fn unavailable_when_account_disabled() {
        let (_tmp, pool) = temp_pool().await;
        set_account(&pool, Some(100.0), Some("monthly")).await;
        sqlx::query("UPDATE platform_account SET enabled = 0 WHERE platform = 'mimo'")
            .execute(&pool)
            .await
            .unwrap();
        let err = MimoAdapter::snapshot(&pool).await.unwrap_err();
        assert!(matches!(err, AdapterError::Unavailable(_)));
    }

    #[tokio::test]
    async fn unavailable_on_unsupported_cycle() {
        let (_tmp, pool) = temp_pool().await;
        set_account(&pool, Some(100.0), Some("weekly")).await;
        let err = MimoAdapter::snapshot(&pool).await.unwrap_err();
        assert!(matches!(err, AdapterError::Unavailable(_)));
    }

    #[tokio::test]
    async fn zero_usage_yields_full_remaining() {
        let (_tmp, pool) = temp_pool().await;
        set_account(&pool, Some(100.0), Some("monthly")).await;
        // 只有一条无 cost 的事件：不参与成本推算
        add_event(&pool, month_start_ms() + 1000, None).await;

        let snap = MimoAdapter::snapshot(&pool).await.unwrap();
        assert_eq!(snap.platform, Platform::Mimo);
        assert_eq!(snap.total, Some(100.0));
        assert_eq!(snap.used, Some(0.0));
        assert_eq!(snap.remaining, Some(100.0));
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert_eq!(detail["progress"], 0.0);
        assert_eq!(detail["quota_cycle"], "monthly");
    }

    #[tokio::test]
    async fn sums_cost_within_current_cycle() {
        let (_tmp, pool) = temp_pool().await;
        set_account(&pool, Some(100.0), Some("monthly")).await;
        add_event(&pool, month_start_ms() + 1000, Some(12.5)).await;
        add_event(&pool, month_start_ms() + 2000, Some(7.5)).await;

        let snap = MimoAdapter::snapshot(&pool).await.unwrap();
        assert_eq!(snap.used, Some(20.0));
        assert_eq!(snap.remaining, Some(80.0));
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert_eq!(detail["progress"], 0.2);
    }

    #[tokio::test]
    async fn over_usage_goes_negative_remaining() {
        let (_tmp, pool) = temp_pool().await;
        set_account(&pool, Some(10.0), Some("monthly")).await;
        add_event(&pool, month_start_ms() + 1000, Some(15.0)).await;

        let snap = MimoAdapter::snapshot(&pool).await.unwrap();
        assert_eq!(snap.used, Some(15.0));
        // 超量：remaining 为负、进度 > 1，不截断
        assert_eq!(snap.remaining, Some(-5.0));
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert_eq!(detail["progress"], 1.5);
    }

    #[tokio::test]
    async fn previous_cycle_events_excluded() {
        let (_tmp, pool) = temp_pool().await;
        set_account(&pool, Some(100.0), Some("monthly")).await;
        // 上个计费周期（本月初前一毫秒）的消耗不计入
        add_event(&pool, month_start_ms() - 1, Some(999.0)).await;
        add_event(&pool, month_start_ms() + 1000, Some(10.0)).await;

        let snap = MimoAdapter::snapshot(&pool).await.unwrap();
        assert_eq!(snap.used, Some(10.0));
        assert_eq!(snap.remaining, Some(90.0));
    }

    #[tokio::test]
    async fn other_platform_events_isolated() {
        let (_tmp, pool) = temp_pool().await;
        set_account(&pool, Some(100.0), Some("monthly")).await;
        add_event(&pool, month_start_ms() + 1000, Some(10.0)).await;
        // kimi 平台的事件不影响 mimo 推算
        sqlx::query(
            "INSERT INTO usage_event
                 (platform, cli, log_file_id, ts, session_id, project, model,
                  input_other, output, cache_read, cache_creation, cost)
             SELECT 'kimi', 'kimi-code', id, ?, 's2', 'app', 'm1', 1, 1, 0, 0, 50.0
             FROM log_file WHERE platform = 'mimo'",
        )
        .bind(month_start_ms() + 1000)
        .execute(&pool)
        .await
        .unwrap();

        let snap = MimoAdapter::snapshot(&pool).await.unwrap();
        assert_eq!(snap.used, Some(10.0));
    }
}
