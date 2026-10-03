//! 平台账户配置（platform_account 表）与配额快照（quota_snapshot 表）的读写。
//! 凭据本体只进 keyring（SecretStore），表里只存 credential_ref 引用名。

use sqlx::sqlite::SqlitePool;

use crate::adapters::kimi;
use crate::keystore::SecretStore;
use crate::kimicode::KIMI_CREDENTIAL_REF;
use crate::models::{
    CommandError, Platform, PlatformAccountInfo, PlatformAccountInput, QuotaSnapshot,
};

/// 账户表行（内部类型；对前端暴露的是 PlatformAccountInfo，has_credential 需查 keyring 补充）
#[derive(Debug, Clone)]
pub struct AccountRow {
    pub id: i64,
    pub platform: Platform,
    pub name: String,
    pub api_base: Option<String>,
    pub credential_ref: Option<String>,
    pub quota_total: Option<f64>,
    pub quota_cycle: Option<String>,
    pub enabled: bool,
}

/// 平台的默认 credential_ref（新建账户行时使用；kimi 沿用旧引用名保证 keyring 既有凭据不失效）
pub fn default_credential_ref(platform: Platform) -> &'static str {
    match platform {
        Platform::Kimi => KIMI_CREDENTIAL_REF,
    }
}

/// 平台的默认 api_base（账户行未配置时的兜底）
pub fn default_api_base(platform: Platform) -> Option<&'static str> {
    match platform {
        Platform::Kimi => Some(kimi::DEFAULT_API_BASE),
    }
}

fn default_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Kimi => "Kimi 开放平台",
    }
}

/// 读取平台账户行（个人自用场景每平台至多一条，取最早配置的）
pub async fn get_row(pool: &SqlitePool, platform: Platform) -> sqlx::Result<Option<AccountRow>> {
    type Row = (
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<f64>,
        Option<String>,
        i64,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, name, api_base, credential_ref, quota_total, quota_cycle, enabled
         FROM platform_account WHERE platform = ? ORDER BY id LIMIT 1",
    )
    .bind(platform.as_str())
    .fetch_optional(pool)
    .await?;
    // WHERE 已按 platform 过滤，返回行的 platform 即入参
    Ok(row.map(
        |(id, name, api_base, credential_ref, quota_total, quota_cycle, enabled)| AccountRow {
            id,
            platform,
            name,
            api_base,
            credential_ref,
            quota_total,
            quota_cycle,
            enabled: enabled != 0,
        },
    ))
}

/// 组装账户视图（platform_account_get 命令）：has_credential 查 keyring 补充
pub async fn get_info(
    pool: &SqlitePool,
    secrets: &dyn SecretStore,
    platform: Platform,
) -> Result<Option<PlatformAccountInfo>, CommandError> {
    let Some(row) = get_row(pool, platform).await? else {
        return Ok(None);
    };
    let has_credential = match &row.credential_ref {
        Some(r) => secrets.get(r)?.is_some(),
        None => false,
    };
    Ok(Some(PlatformAccountInfo {
        id: row.id,
        platform: row.platform,
        name: row.name,
        api_base: row.api_base,
        credential_ref: row.credential_ref,
        has_credential,
        quota_total: row.quota_total,
        quota_cycle: row.quota_cycle,
        enabled: row.enabled,
    }))
}

/// 保存平台账户配置（platform_account_set 命令的业务部分）：
/// 每平台单行 upsert——已存在则更新（保留原 credential_ref 不动 keyring 关联），
/// 不存在则插入（credential_ref 取平台默认引用名）。凭据本体由调用方另行写入 keyring。
pub async fn set_row(
    pool: &SqlitePool,
    platform: Platform,
    input: &PlatformAccountInput,
) -> Result<(), CommandError> {
    let name = if input.name.is_empty() {
        default_name(platform).to_string()
    } else {
        input.name.clone()
    };
    // api_base 空串视为未配置（取默认 base 的逻辑在使用侧兜底）
    let api_base = input
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    match get_row(pool, platform).await? {
        Some(row) => {
            sqlx::query(
                "UPDATE platform_account
                 SET name = ?, api_base = ?, quota_total = ?, quota_cycle = ?, enabled = ?
                 WHERE id = ?",
            )
            .bind(name)
            .bind(api_base)
            .bind(input.quota_total)
            .bind(&input.quota_cycle)
            .bind(input.enabled as i64)
            .bind(row.id)
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query(
                "INSERT INTO platform_account
                     (platform, name, api_base, credential_ref, quota_total, quota_cycle, enabled)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(platform.as_str())
            .bind(name)
            .bind(api_base)
            .bind(default_credential_ref(platform))
            .bind(input.quota_total)
            .bind(&input.quota_cycle)
            .bind(input.enabled as i64)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

/// 写入一条配额快照（只追加，天然是历史）
pub async fn save_quota_snapshot(pool: &SqlitePool, snapshot: &QuotaSnapshot) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO quota_snapshot (platform, total, used, remaining, detail_json, fetched_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(snapshot.platform.as_str())
    .bind(snapshot.total)
    .bind(snapshot.used)
    .bind(snapshot.remaining)
    .bind(&snapshot.detail_json)
    .bind(&snapshot.fetched_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// 读取最近一条配额快照（quota_latest 命令；不触发网络请求）
pub async fn latest_quota_snapshot(
    pool: &SqlitePool,
    platform: Platform,
) -> sqlx::Result<Option<QuotaSnapshot>> {
    type Row = (
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<String>,
        String,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT total, used, remaining, detail_json, fetched_at
         FROM quota_snapshot WHERE platform = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(platform.as_str())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(total, used, remaining, detail_json, fetched_at)| QuotaSnapshot {
            platform,
            total,
            used,
            remaining,
            detail_json,
            fetched_at,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::MemoryStore;
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

    fn input(name: &str) -> PlatformAccountInput {
        PlatformAccountInput {
            platform: Platform::Kimi,
            name: name.to_string(),
            api_base: None,
            quota_total: None,
            quota_cycle: None,
            enabled: true,
        }
    }

    #[tokio::test]
    async fn account_upsert_roundtrip_preserves_credential_ref() {
        let (_tmp, pool) = temp_pool().await;
        let secrets = MemoryStore::default();
        assert!(get_info(&pool, &secrets, Platform::Kimi)
            .await
            .unwrap()
            .is_none());

        // 新建：credential_ref 取平台默认（kimi 沿用旧引用名，保证 keyring 既有凭据不失效）
        set_row(&pool, Platform::Kimi, &input("主账号"))
            .await
            .unwrap();
        let info = get_info(&pool, &secrets, Platform::Kimi)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(info.name, "主账号");
        assert_eq!(info.credential_ref.as_deref(), Some(KIMI_CREDENTIAL_REF));
        assert!(!info.has_credential);

        // keyring 写入凭据后 has_credential 翻转
        secrets.set(KIMI_CREDENTIAL_REF, "sk-x").unwrap();
        let info = get_info(&pool, &secrets, Platform::Kimi)
            .await
            .unwrap()
            .unwrap();
        assert!(info.has_credential);

        // 更新：仍单行、credential_ref 不被覆盖
        set_row(&pool, Platform::Kimi, &input("改名"))
            .await
            .unwrap();
        let rows: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM platform_account WHERE platform = 'kimi'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rows.0, 1);
        let info = get_info(&pool, &secrets, Platform::Kimi)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(info.name, "改名");
        assert_eq!(info.credential_ref.as_deref(), Some(KIMI_CREDENTIAL_REF));

        // 空 name 回退默认名
        set_row(&pool, Platform::Kimi, &input("")).await.unwrap();
        let info = get_info(&pool, &secrets, Platform::Kimi)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(info.name, "Kimi 开放平台");
    }

    #[tokio::test]
    async fn quota_snapshot_save_and_latest() {
        let (_tmp, pool) = temp_pool().await;
        assert!(latest_quota_snapshot(&pool, Platform::Kimi)
            .await
            .unwrap()
            .is_none());

        let snap1 = QuotaSnapshot {
            platform: Platform::Kimi,
            total: None,
            used: None,
            remaining: Some(10.0),
            detail_json: None,
            fetched_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let snap2 = QuotaSnapshot {
            remaining: Some(9.5),
            fetched_at: "2026-01-02T00:00:00Z".to_string(),
            ..snap1.clone()
        };
        save_quota_snapshot(&pool, &snap1).await.unwrap();
        save_quota_snapshot(&pool, &snap2).await.unwrap();

        let latest = latest_quota_snapshot(&pool, Platform::Kimi)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest, snap2);
    }
}
