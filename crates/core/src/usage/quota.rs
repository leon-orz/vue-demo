//! 配额/余额刷新编排（quota_refresh 命令的业务层）：
//! 查 platform_account → keystore 取凭据 → 调对应平台适配器 → 写 quota_snapshot 表。
//! 单平台失败返回 CommandError（不 panic）；Kimi 的 coding usages 为非官方 best-effort 接口，
//! 失败只记入 detail_json，不拖垮官方余额主数据源。

use sqlx::sqlite::SqlitePool;

use super::account::{self, AccountRow};
use crate::adapters::deepseek::DeepSeekAdapter;
use crate::adapters::kimi::{KimiAdapter, DEFAULT_CODING_API_BASE};
use crate::adapters::mimo::MimoAdapter;
use crate::adapters::{AdapterError, UsageAdapter};
use crate::keystore::SecretStore;
use crate::models::{CommandError, Credential, Platform, QuotaSnapshot};

/// 刷新指定平台配额/余额并写快照，返回写入的快照。
/// Kimi 一次 refresh 只写一条快照（两个数据源的取舍见 merge_kimi_snapshot 注释）。
pub async fn refresh(
    pool: &SqlitePool,
    secrets: &dyn SecretStore,
    platform: Platform,
) -> Result<QuotaSnapshot, CommandError> {
    refresh_with(pool, secrets, platform, None).await
}

/// coding_base_override 仅供测试注入 mock server；生产为 None（用默认常量）
async fn refresh_with(
    pool: &SqlitePool,
    secrets: &dyn SecretStore,
    platform: Platform,
    coding_base_override: Option<&str>,
) -> Result<QuotaSnapshot, CommandError> {
    let snapshot = match platform {
        // MiMo 纯本地推算（读 platform_account + usage_event.cost），不需要凭据
        Platform::Mimo => MimoAdapter::snapshot(pool).await?,
        Platform::Kimi => {
            let (account, key) = require_account(pool, secrets, platform).await?;
            refresh_kimi(&account, &key, coding_base_override).await?
        }
        Platform::Deepseek => {
            let (account, key) = require_account(pool, secrets, platform).await?;
            refresh_deepseek(&account, &key).await?
        }
        // omp 只有本地日志，无配额/余额数据源
        Platform::Omp => {
            return Err(CommandError::unavailable(
                "omp 平台无配额/余额数据源（仅本地日志统计）",
            ))
        }
    };
    account::save_quota_snapshot(pool, &snapshot).await?;
    Ok(snapshot)
}

/// 取账户并校验：未配置 → not_found；已停用 → unavailable；凭据缺失 → not_found。
/// 返回账户行与 keyring 中取出的凭据。
async fn require_account(
    pool: &SqlitePool,
    secrets: &dyn SecretStore,
    platform: Platform,
) -> Result<(AccountRow, String), CommandError> {
    let account = account::get_row(pool, platform)
        .await?
        .ok_or_else(|| CommandError::not_found(format!("尚未配置 {platform} 平台账户")))?;
    if !account.enabled {
        return Err(CommandError::unavailable(format!(
            "{platform} 平台账户已停用"
        )));
    }
    let cred_ref = account
        .credential_ref
        .clone()
        .ok_or_else(|| CommandError::not_found(format!("{platform} 平台账户未配置凭据引用")))?;
    let key = secrets
        .get(&cred_ref)?
        .ok_or_else(|| CommandError::not_found("keyring 中未找到 API key，请先在设置中配置"))?;
    Ok((account, key))
}

/// Kimi 配额刷新：官方余额为主（失败即整体失败），coding usages best-effort（失败降级进 detail_json）。
async fn refresh_kimi(
    account: &AccountRow,
    key: &str,
    coding_base_override: Option<&str>,
) -> Result<QuotaSnapshot, CommandError> {
    let api_base = account
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(crate::adapters::kimi::DEFAULT_API_BASE);
    let adapter = KimiAdapter::new();
    let balance = adapter.fetch_balance(api_base, key).await?;
    let coding_base = coding_base_override.unwrap_or(DEFAULT_CODING_API_BASE);
    let coding = adapter.fetch_coding_usages(coding_base, key).await;
    Ok(merge_kimi_snapshot(&balance, coding))
}

/// 合并 Kimi 两个数据源为一条快照（取舍说明）：
/// 余额（/v1/users/me/balance）与套餐用量（/coding/v1/usages，非官方 best-effort）
/// 语义不同——前者是按量金额，后者是 5h 窗口 + 月度池余量。quota_snapshot 表按平台一行，
/// 拆两条会让 quota_latest 的"最近一条"语义模糊，故合并为一条：
/// coding 月度池可用时 total/used/remaining 取自它（套餐余量是用户更关心的进度），
/// 否则 remaining 回落为余额 available；两个数据源的完整明细都进 detail_json.sources。
fn merge_kimi_snapshot(
    balance: &crate::adapters::kimi::KimiBalance,
    coding: Result<crate::adapters::kimi::KimiCodingUsages, AdapterError>,
) -> QuotaSnapshot {
    let (total, used, remaining, coding_json, coding_error) = match coding {
        Ok(u) => (
            u.month_total,
            u.month_used,
            u.month_remaining.or(Some(balance.available)),
            serde_json::json!({
                "month_total": u.month_total,
                "month_used": u.month_used,
                "month_remaining": u.month_remaining,
                "window_used_percent": u.window_used_percent,
                "window_remaining_percent": u.window_remaining_percent,
                "window_reset_at": u.window_reset_at,
            }),
            None,
        ),
        Err(e) => (
            None,
            None,
            Some(balance.available),
            serde_json::Value::Null,
            Some(e.to_string()),
        ),
    };
    QuotaSnapshot {
        platform: Platform::Kimi,
        total,
        used,
        remaining,
        detail_json: Some(
            serde_json::json!({
                "sources": {
                    "balance": {
                        "available": balance.available,
                        "voucher": balance.voucher,
                        "cash": balance.cash,
                        "currency": "cny",
                    },
                    "coding_usages": coding_json,
                    "coding_usages_error": coding_error,
                }
            })
            .to_string(),
        ),
        fetched_at: balance.fetched_at.clone(),
    }
}

/// DeepSeek 余额刷新（官方 /user/balance）
async fn refresh_deepseek(account: &AccountRow, key: &str) -> Result<QuotaSnapshot, CommandError> {
    let api_base = account
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(crate::adapters::deepseek::DEFAULT_API_BASE);
    let snapshot = DeepSeekAdapter::new()
        .fetch(api_base, &Credential::ApiKey(key.to_string()))
        .await?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::MemoryStore;
    use crate::models::PlatformAccountInput;
    use crate::usage::account::{default_credential_ref, set_row};
    use sqlx::sqlite::SqliteConnectOptions;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn temp_pool() -> (tempfile::TempDir, SqlitePool) {
        let tmp = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(tmp.path().join("t.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();
        (tmp, pool)
    }

    fn input(platform: Platform, api_base: Option<String>) -> PlatformAccountInput {
        PlatformAccountInput {
            platform,
            name: "测试账户".to_string(),
            api_base,
            quota_total: None,
            quota_cycle: None,
            enabled: true,
        }
    }

    fn balance_body() -> serde_json::Value {
        serde_json::json!({
            "data": {"available_balance": 10.5, "voucher_balance": 2.0, "cash_balance": 8.5}
        })
    }

    fn usages_body() -> serde_json::Value {
        serde_json::json!({
            "data": {
                "window": {"used_percent": 12.5, "remaining_percent": 87.5, "reset_at": "2026-09-21T15:00:00Z"},
                "month": {"total": 1000.0, "used": 300.0, "remaining": 700.0}
            }
        })
    }

    /// 配置 kimi 账户（api_base 指向 mock server）并把 key 写进 MemoryStore
    async fn setup_kimi(pool: &SqlitePool, secrets: &MemoryStore, base: &str) {
        set_row(
            pool,
            Platform::Kimi,
            &input(Platform::Kimi, Some(base.to_string())),
        )
        .await
        .unwrap();
        secrets
            .set(default_credential_ref(Platform::Kimi).unwrap(), "sk-test")
            .unwrap();
    }

    async fn snapshot_count(pool: &SqlitePool, platform: &str) -> i64 {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM quota_snapshot WHERE platform = ?")
            .bind(platform)
            .fetch_one(pool)
            .await
            .unwrap();
        n
    }

    #[tokio::test]
    async fn kimi_refresh_merges_balance_and_coding_usages() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/users/me/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(balance_body()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/coding/v1/usages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(usages_body()))
            .mount(&server)
            .await;
        setup_kimi(&pool, &secrets, &server.uri()).await;

        let snap = refresh_with(&pool, &secrets, Platform::Kimi, Some(&server.uri()))
            .await
            .unwrap();
        // coding 月度池可用：total/used/remaining 取自它
        assert_eq!(snap.total, Some(1000.0));
        assert_eq!(snap.used, Some(300.0));
        assert_eq!(snap.remaining, Some(700.0));
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert_eq!(detail["sources"]["balance"]["available"], 10.5);
        assert_eq!(detail["sources"]["balance"]["voucher"], 2.0);
        assert_eq!(detail["sources"]["coding_usages"]["month_total"], 1000.0);
        assert_eq!(
            detail["sources"]["coding_usages"]["window_used_percent"],
            12.5
        );
        assert!(detail["sources"]["coding_usages_error"].is_null());

        // 已落 quota_snapshot，latest 读得回
        assert_eq!(snapshot_count(&pool, "kimi").await, 1);
        let latest = account::latest_quota_snapshot(&pool, Platform::Kimi)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.remaining, Some(700.0));
    }

    #[tokio::test]
    async fn kimi_refresh_coding_failure_degrades_to_detail_error() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/users/me/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(balance_body()))
            .mount(&server)
            .await;
        // coding usages 接口失效（best-effort）：500
        Mock::given(method("GET"))
            .and(path("/coding/v1/usages"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        setup_kimi(&pool, &secrets, &server.uri()).await;

        let snap = refresh_with(&pool, &secrets, Platform::Kimi, Some(&server.uri()))
            .await
            .unwrap();
        // 余额主数据源仍成功：remaining 回落为 available
        assert_eq!(snap.total, None);
        assert_eq!(snap.remaining, Some(10.5));
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert!(detail["sources"]["coding_usages"].is_null());
        assert!(detail["sources"]["coding_usages_error"].is_string());
        assert_eq!(snapshot_count(&pool, "kimi").await, 1);
    }

    #[tokio::test]
    async fn kimi_refresh_balance_unauthorized_fails_whole_refresh() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        setup_kimi(&pool, &secrets, &server.uri()).await;

        let err = refresh_with(&pool, &secrets, Platform::Kimi, Some(&server.uri()))
            .await
            .unwrap_err();
        assert_eq!(err.kind, "unauthorized");
        // 失败不落快照
        assert_eq!(snapshot_count(&pool, "kimi").await, 0);
    }

    #[tokio::test]
    async fn refresh_missing_account_or_key() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        // 未配置账户
        let err = refresh(&pool, &secrets, Platform::Kimi).await.unwrap_err();
        assert_eq!(err.kind, "not_found");

        // 配置了账户但 keyring 无 key
        set_row(&pool, Platform::Kimi, &input(Platform::Kimi, None))
            .await
            .unwrap();
        let err = refresh(&pool, &secrets, Platform::Kimi).await.unwrap_err();
        assert_eq!(err.kind, "not_found");

        // 账户停用
        let mut disabled = input(Platform::Kimi, None);
        disabled.enabled = false;
        set_row(&pool, Platform::Kimi, &disabled).await.unwrap();
        secrets
            .set(default_credential_ref(Platform::Kimi).unwrap(), "sk-test")
            .unwrap();
        let err = refresh(&pool, &secrets, Platform::Kimi).await.unwrap_err();
        assert_eq!(err.kind, "unavailable");
    }

    #[tokio::test]
    async fn deepseek_refresh_success_and_error_path() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "is_available": true,
                "balance_infos": [{"currency": "CNY", "total_balance": "110.00"}]
            })))
            .mount(&server)
            .await;
        set_row(
            &pool,
            Platform::Deepseek,
            &input(Platform::Deepseek, Some(server.uri())),
        )
        .await
        .unwrap();
        secrets
            .set(default_credential_ref(Platform::Deepseek).unwrap(), "sk-ds")
            .unwrap();

        let snap = refresh(&pool, &secrets, Platform::Deepseek).await.unwrap();
        assert_eq!(snap.remaining, Some(110.0));
        assert_eq!(snapshot_count(&pool, "deepseek").await, 1);

        // 401 → unauthorized，不再追加快照
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let err = refresh(&pool, &secrets, Platform::Deepseek)
            .await
            .unwrap_err();
        assert_eq!(err.kind, "unauthorized");
        assert_eq!(snapshot_count(&pool, "deepseek").await, 1);
    }

    #[tokio::test]
    async fn mimo_refresh_is_local_estimation() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        let mut input = input(Platform::Mimo, None);
        input.quota_total = Some(100.0);
        input.quota_cycle = Some("monthly".to_string());
        set_row(&pool, Platform::Mimo, &input).await.unwrap();

        // 无网络、无凭据：本地推算成功落快照
        let snap = refresh(&pool, &secrets, Platform::Mimo).await.unwrap();
        assert_eq!(snap.total, Some(100.0));
        assert_eq!(snap.used, Some(0.0));
        assert_eq!(snapshot_count(&pool, "mimo").await, 1);

        // 未手录总额 → unavailable
        sqlx::query("UPDATE platform_account SET quota_total = NULL WHERE platform = 'mimo'")
            .execute(&pool)
            .await
            .unwrap();
        let err = refresh(&pool, &secrets, Platform::Mimo).await.unwrap_err();
        assert_eq!(err.kind, "unavailable");
    }

    #[tokio::test]
    async fn omp_has_no_quota_source() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        let err = refresh(&pool, &secrets, Platform::Omp).await.unwrap_err();
        assert_eq!(err.kind, "unavailable");
    }

    // ---- detail_json 前后端契约锁 ----
    // 前端 apps/desktop/src/stores/usage.ts 的 parseKimi/Mimo/DeepseekQuotaDetail 按固定
    // 键名解析 detail_json；Rust 改键名不会报错，前端只会静默降级（字段解析为 null）。
    // 以下测试锁死三个平台 detail_json 的键集合与值类型，改形状必须先改前端。

    /// 断言 JSON 对象的键集合与期望完全一致（多键/少键/改键名都失败）
    fn assert_json_keys(value: &serde_json::Value, mut expected: Vec<&str>) {
        let obj = value.as_object().expect("应为 JSON 对象");
        let mut actual: Vec<&str> = obj.keys().map(String::as_str).collect();
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected, "detail_json 键集合与前端契约不一致");
    }

    #[tokio::test]
    async fn kimi_detail_json_contract_locked() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/users/me/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(balance_body()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/coding/v1/usages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(usages_body()))
            .mount(&server)
            .await;
        setup_kimi(&pool, &secrets, &server.uri()).await;

        let snap = refresh_with(&pool, &secrets, Platform::Kimi, Some(&server.uri()))
            .await
            .unwrap();
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert_json_keys(&detail, vec!["sources"]);
        let sources = &detail["sources"];
        assert_json_keys(
            sources,
            vec!["balance", "coding_usages", "coding_usages_error"],
        );

        // sources.balance：开放平台按量余额分项（单位元）
        let balance = &sources["balance"];
        assert_json_keys(balance, vec!["available", "voucher", "cash", "currency"]);
        assert!(balance["available"].is_number());
        assert!(balance["voucher"].is_number());
        assert!(balance["cash"].is_number());
        assert!(balance["currency"].is_string());

        // sources.coding_usages：套餐 5h 滚动窗口 + 月度池（字段可空，键名固定）
        let coding = &sources["coding_usages"];
        assert_json_keys(
            coding,
            vec![
                "month_total",
                "month_used",
                "month_remaining",
                "window_used_percent",
                "window_remaining_percent",
                "window_reset_at",
            ],
        );
        assert!(coding["month_total"].is_number());
        assert!(coding["month_used"].is_number());
        assert!(coding["month_remaining"].is_number());
        assert!(coding["window_used_percent"].is_number());
        assert!(coding["window_remaining_percent"].is_number());
        assert!(coding["window_reset_at"].is_string());

        // 成功路径无降级原因
        assert!(sources["coding_usages_error"].is_null());
    }

    #[tokio::test]
    async fn kimi_detail_json_contract_locked_on_coding_failure() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/users/me/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(balance_body()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/coding/v1/usages"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        setup_kimi(&pool, &secrets, &server.uri()).await;

        let snap = refresh_with(&pool, &secrets, Platform::Kimi, Some(&server.uri()))
            .await
            .unwrap();
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        // 降级路径键集合不变：coding_usages 为 null、coding_usages_error 为字符串
        assert_json_keys(&detail, vec!["sources"]);
        let sources = &detail["sources"];
        assert_json_keys(
            sources,
            vec!["balance", "coding_usages", "coding_usages_error"],
        );
        assert_json_keys(
            &sources["balance"],
            vec!["available", "voucher", "cash", "currency"],
        );
        assert!(sources["coding_usages"].is_null());
        assert!(sources["coding_usages_error"].is_string());
    }

    #[tokio::test]
    async fn mimo_detail_json_contract_locked() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        let mut input = input(Platform::Mimo, None);
        input.quota_total = Some(100.0);
        input.quota_cycle = Some("monthly".to_string());
        set_row(&pool, Platform::Mimo, &input).await.unwrap();

        let snap = refresh(&pool, &secrets, Platform::Mimo).await.unwrap();
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert_json_keys(
            &detail,
            vec!["quota_cycle", "period_start", "progress", "unit"],
        );
        assert_eq!(detail["quota_cycle"], "monthly");
        assert!(detail["period_start"].is_string());
        assert!(detail["progress"].is_number());
        assert_eq!(detail["unit"], "credit");
    }

    #[tokio::test]
    async fn deepseek_detail_json_contract_locked() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "is_available": true,
                "balance_infos": [{
                    "currency": "CNY",
                    "total_balance": "110.00",
                    "granted_balance": "10.00",
                    "topped_up_balance": "100.00"
                }]
            })))
            .mount(&server)
            .await;
        set_row(
            &pool,
            Platform::Deepseek,
            &input(Platform::Deepseek, Some(server.uri())),
        )
        .await
        .unwrap();
        secrets
            .set(default_credential_ref(Platform::Deepseek).unwrap(), "sk-ds")
            .unwrap();

        let snap = refresh(&pool, &secrets, Platform::Deepseek).await.unwrap();
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert_json_keys(&detail, vec!["is_available", "balances"]);
        assert!(detail["is_available"].is_boolean());
        // balances 为币种分项数组，元素键名固定（granted/topped_up 可空）
        let items = detail["balances"].as_array().expect("balances 应为数组");
        assert!(!items.is_empty());
        let item = &items[0];
        assert_json_keys(item, vec!["currency", "total", "granted", "topped_up"]);
        assert!(item["currency"].is_string());
        assert!(item["total"].is_number());
        assert!(item["granted"].is_number());
        assert!(item["topped_up"].is_number());
    }
}
