//! 当前工具的 SQLite schema 迁移，历史 SQL 只保留归档，不再执行废弃业务的建表和回填。
//! 迁移版本只增不改，每条迁移的语句与版本登记在同一事务中提交。
//! 文件按分号切分执行，SQL 字符串字面量内禁止使用分号。

use sqlx::sqlite::SqlitePool;

/// 一条迁移：version 全局唯一，保留现有业务使用的历史版本号。
pub struct Migration {
    pub version: i64,
    pub description: &'static str,
    pub sql: &'static str,
}

/// 当前业务迁移；已下线工具的旧迁移不再运行，避免依赖已删除的旧表。
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 5,
        description: "create_proxy_setting",
        sql: include_str!("../migrations/0005_proxy_setting.sql"),
    },
    Migration {
        version: 8,
        description: "create_preferences",
        sql: include_str!("../migrations/0008_preferences.sql"),
    },
    Migration {
        version: 9,
        description: "remove_retired_tools",
        sql: include_str!("../migrations/0009_remove_retired_tools.sql"),
    },
];

/// 执行当前工具尚未应用的迁移；即使旧库已登记版本，也重复执行幂等 SQL，修复登记与实际表不一致。
/// 每条迁移在独立事务中执行，完成后登记版本。
pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    for migration in MIGRATIONS {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _toogl_migrations (
                version INTEGER PRIMARY KEY,
                description TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&mut *tx)
        .await?;
        // 历史库可能只登记版本但缺少实际表，因此不能仅凭版本行跳过 SQL。
        // 所有登记脚本都须可重放；新增一次性数据迁移前先调整迁移协议。
        for stmt in migration.sql.split(';') {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt).execute(&mut *tx).await?;
            }
        }
        let applied: Option<(i64,)> =
            sqlx::query_as("SELECT version FROM _toogl_migrations WHERE version = ?")
                .bind(migration.version)
                .fetch_optional(&mut *tx)
                .await?;
        if applied.is_none() {
            sqlx::query("INSERT INTO _toogl_migrations (version, description) VALUES (?, ?)")
                .bind(migration.version)
                .bind(migration.description)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
    }
    Ok(())
}
