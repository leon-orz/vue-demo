use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use toogl_core::{
    models::ThemeMode, preferences::PreferenceStore, proxy_setting::ProxySettingStore,
};

async fn pool_at(path: &std::path::Path) -> SqlitePool {
    SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn upgrade_partial_history_preserves_current_settings() {
    // 旧迁移登记与实际表不一致时，当前业务仍须能初始化且保留数据。
    for applied_through in 0..=8 {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("toogl.db");
        let pool = pool_at(&path).await;
        sqlx::query(
            "CREATE TABLE _toogl_migrations (
                version INTEGER PRIMARY KEY,
                description TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        for version in 1..=applied_through {
            sqlx::query(
                "INSERT INTO _toogl_migrations (version, description) VALUES (?, '旧版本')",
            )
            .bind(version)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(include_str!("../migrations/0005_proxy_setting.sql"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(include_str!("../migrations/0008_preferences.sql"))
            .execute(&pool)
            .await
            .unwrap();
        for (key, value) in [
            ("subscription_url", "https://example.com/sub"),
            ("manual_nodes", "[{\"name\":\"手动节点\"}]"),
            ("system_proxy_backup", "{\"enabled\":false}"),
            ("controller_secret", "test-secret"),
        ] {
            sqlx::query("INSERT INTO proxy_setting (key, value) VALUES (?, ?)")
                .bind(key)
                .bind(value)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO preference VALUES ('app', '{\"theme\":\"dark\"}')")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let preferences = PreferenceStore::connect(&path).await.unwrap();
        assert_eq!(
            preferences.app().await.unwrap().unwrap().theme,
            ThemeMode::Dark
        );
        let proxy = ProxySettingStore::connect(&path).await.unwrap();
        assert_eq!(
            proxy.get("subscription_url").await.unwrap().as_deref(),
            Some("https://example.com/sub")
        );
        assert_eq!(
            proxy.get("manual_nodes").await.unwrap().as_deref(),
            Some("[{\"name\":\"手动节点\"}]")
        );
        assert_eq!(
            proxy.get("system_proxy_backup").await.unwrap().as_deref(),
            Some("{\"enabled\":false}")
        );
        assert_eq!(
            proxy.get("controller_secret").await.unwrap().as_deref(),
            Some("test-secret")
        );
        let pool = pool_at(&path).await;
        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            tables,
            vec![
                ("_toogl_migrations".to_string(),),
                ("preference".to_string(),),
                ("proxy_setting".to_string(),),
            ],
            "旧版本 {applied_through} 升级后不应重建废弃表"
        );
        pool.close().await;
    }
}

#[tokio::test]
async fn retired_tables_are_removed_with_foreign_keys_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("toogl.db");
    let pool = pool_at(&path).await;
    // 包含真实外键和数据，防止清理时先删父表导致升级失败。
    for sql in [
        "CREATE TABLE kimi_account (id INTEGER PRIMARY KEY)",
        "CREATE TABLE kimi_log_file (id INTEGER PRIMARY KEY)",
        "CREATE TABLE kimi_usage_event (log_file_id INTEGER REFERENCES kimi_log_file(id))",
        "CREATE TABLE kimi_usage_daily (id INTEGER PRIMARY KEY)",
        "CREATE TABLE kimi_balance_snapshot (id INTEGER PRIMARY KEY)",
        "CREATE TABLE log_file (id INTEGER PRIMARY KEY)",
        "CREATE TABLE usage_event (log_file_id INTEGER REFERENCES log_file(id))",
        "CREATE TABLE usage_daily (id INTEGER PRIMARY KEY)",
        "CREATE TABLE quota_snapshot (id INTEGER PRIMARY KEY)",
        "CREATE TABLE platform_account (id INTEGER PRIMARY KEY)",
        "CREATE TABLE platform (id INTEGER PRIMARY KEY)",
        "CREATE TABLE usage_snapshot (platform_id INTEGER REFERENCES platform(id))",
        "INSERT INTO kimi_log_file VALUES (1)",
        "INSERT INTO kimi_usage_event VALUES (1)",
        "INSERT INTO log_file VALUES (1)",
        "INSERT INTO usage_event VALUES (1)",
        "INSERT INTO platform VALUES (1)",
        "INSERT INTO usage_snapshot VALUES (1)",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    toogl_core::db::migrate(&pool).await.unwrap();
    toogl_core::db::migrate(&pool).await.unwrap();
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        tables,
        vec![
            ("_toogl_migrations".to_string(),),
            ("preference".to_string(),),
            ("proxy_setting".to_string(),),
        ]
    );
    pool.close().await;
}
