//! 通用 SQLite schema 迁移（schema 管理统一收归 Rust 侧，tauri-plugin-sql 已移除）。
//! 约定：
//! - SQL 文件放在 crates/core/migrations/，命名 NNNN_description.sql，版本号严格递增、只增不改；
//! - 每个文件按分号切分逐条执行，因此禁止在字符串字面量中使用分号；
//! - 所有语句必须幂等（IF NOT EXISTS 等），以兼容历史上未经迁移表登记的既有数据库。

use std::collections::HashSet;

use sqlx::sqlite::SqlitePool;

/// 一条迁移：version 全局唯一且只增不改
pub struct Migration {
    pub version: i64,
    pub description: &'static str,
    pub sql: &'static str,
}

/// 全量迁移列表（新增 migration 时在末尾追加一条）
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "create_platform_and_usage_snapshot",
        sql: include_str!("../migrations/0001_init.sql"),
    },
    Migration {
        version: 2,
        description: "create_kimi_tables",
        sql: include_str!("../migrations/0002_kimi.sql"),
    },
];

/// 按版本号执行未应用过的迁移，并登记到 _toogl_migrations 表。
/// 语句级幂等兜底：即使登记缺失（如历史库），IF NOT EXISTS 也保证重复执行安全。
pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _toogl_migrations (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;
    let rows: Vec<(i64,)> = sqlx::query_as("SELECT version FROM _toogl_migrations")
        .fetch_all(pool)
        .await?;
    let applied: HashSet<i64> = rows.into_iter().map(|(v,)| v).collect();

    for m in MIGRATIONS {
        if applied.contains(&m.version) {
            continue;
        }
        for stmt in m.sql.split(';') {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt).execute(pool).await?;
            }
        }
        sqlx::query("INSERT INTO _toogl_migrations (version, description) VALUES (?, ?)")
            .bind(m.version)
            .bind(m.description)
            .execute(pool)
            .await?;
    }
    Ok(())
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
        (tmp, SqlitePool::connect_with(options).await.unwrap())
    }

    #[tokio::test]
    async fn migrate_is_idempotent_and_registers_versions() {
        let (_tmp, pool) = temp_pool().await;
        migrate(&pool).await.unwrap();
        migrate(&pool).await.unwrap(); // 二次执行不报错

        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM _toogl_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, MIGRATIONS.len() as i64);

        // 0001 / 0002 的表都已建好
        let (platform_ok,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE name = 'platform'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let (kimi_ok,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE name = 'kimi_usage_event'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(platform_ok, 1);
        assert_eq!(kimi_ok, 1);
    }
}
