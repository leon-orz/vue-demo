//! 通用 SQLite schema 迁移（schema 管理统一收归 Rust 侧，tauri-plugin-sql 已移除）。
//! 约定：
//! - SQL 文件放在 crates/core/migrations/，命名 NNNN_description.sql，版本号严格递增、只增不改；
//! - 每个文件按分号切分逐条执行，因此禁止在字符串字面量中使用分号；
//! - 所有语句必须幂等（IF NOT EXISTS 等），以兼容历史上未经迁移表登记的既有数据库；
//!   ALTER TABLE ADD COLUMN 无 IF NOT EXISTS，用 Migration.guard_column 列守卫实现幂等。

use std::collections::HashSet;

use sqlx::sqlite::SqlitePool;

/// 一条迁移：version 全局唯一且只增不改
pub struct Migration {
    pub version: i64,
    pub description: &'static str,
    pub sql: &'static str,
    /// 列级幂等守卫：Some((表, 列)) 时，若该列已存在则整段 SQL 跳过（仍登记版本号）。
    /// 用于 ALTER TABLE ADD COLUMN 这类无 IF NOT EXISTS 的语句（SQLite 不支持）。
    pub guard_column: Option<(&'static str, &'static str)>,
}

/// 全量迁移列表（新增 migration 时在末尾追加一条）
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "create_platform_and_usage_snapshot",
        sql: include_str!("../migrations/0001_init.sql"),
        guard_column: None,
    },
    Migration {
        version: 2,
        description: "create_kimi_tables",
        sql: include_str!("../migrations/0002_kimi.sql"),
        guard_column: None,
    },
    Migration {
        version: 3,
        description: "multiplatform_generalization",
        sql: include_str!("../migrations/0003_multiplatform.sql"),
        guard_column: None,
    },
    Migration {
        version: 4,
        description: "cleanup_legacy_kimi_and_gateway_tables",
        sql: include_str!("../migrations/0004_cleanup_legacy.sql"),
        guard_column: None,
    },
    Migration {
        version: 5,
        description: "create_proxy_setting",
        sql: include_str!("../migrations/0005_proxy_setting.sql"),
        guard_column: None,
    },
    Migration {
        version: 6,
        description: "kimi_only_cleanup",
        sql: include_str!("../migrations/0006_kimi_only.sql"),
        guard_column: None,
    },
    Migration {
        version: 7,
        description: "usage_daily_request_count",
        sql: include_str!("../migrations/0007_usage_daily_request_count.sql"),
        // request_count 已存在（如历史库手工加过列）则整段跳过，保证 ADD COLUMN 幂等
        guard_column: Some(("usage_daily", "request_count")),
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
        // 列守卫：目标列已存在时整段跳过（仍登记版本号），
        // 让 ALTER TABLE ADD COLUMN 这类无 IF NOT EXISTS 的语句在重复执行/历史库上幂等
        if let Some((table, column)) = m.guard_column {
            if column_exists(pool, table, column).await? {
                register_migration(pool, m).await?;
                continue;
            }
        }
        for stmt in m.sql.split(';') {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt).execute(pool).await?;
            }
        }
        register_migration(pool, m).await?;
    }
    Ok(())
}

/// 用 pragma_table_info 判断列是否已存在（表名/列名只来自 MIGRATIONS 常量，无注入面）
async fn column_exists(pool: &SqlitePool, table: &str, column: &str) -> Result<bool, sqlx::Error> {
    let sql = format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?");
    let (count,): (i64,) = sqlx::query_as(&sql).bind(column).fetch_one(pool).await?;
    Ok(count > 0)
}

/// 登记一条已应用的迁移
async fn register_migration(pool: &SqlitePool, m: &Migration) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO _toogl_migrations (version, description) VALUES (?, ?)")
        .bind(m.version)
        .bind(m.description)
        .execute(pool)
        .await?;
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

        // 0004 已清理旧表：0001 的 platform / usage_snapshot 与 0002 的 kimi_* 五表全部 DROP
        for table in [
            "platform",
            "usage_snapshot",
            "kimi_account",
            "kimi_log_file",
            "kimi_usage_event",
            "kimi_usage_daily",
            "kimi_balance_snapshot",
        ] {
            let (ok,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE name = ?")
                .bind(table)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(ok, 0, "旧表 {table} 应已被 0004 清理");
        }

        // 0003 的新表都已建好
        for table in [
            "log_file",
            "usage_event",
            "usage_daily",
            "quota_snapshot",
            "platform_account",
        ] {
            let (ok,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE name = ?")
                .bind(table)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(ok, 1, "缺少表 {table}");
        }
    }

    /// 手动执行某个迁移文件的全部语句（与 migrate 相同的分号切分逻辑）
    async fn apply_sql(pool: &SqlitePool, sql: &str) {
        for stmt in sql.split(';') {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt).execute(pool).await.unwrap();
            }
        }
    }

    /// 0003 回填：旧 kimi_* 表造数 → 迁移 → 新表数据正确（含新旧 id 映射），重复迁移幂等
    #[tokio::test]
    async fn migration_0003_backfills_kimi_data_idempotently() {
        let (_tmp, pool) = temp_pool().await;
        // 模拟既有数据库：只应用 0001/0002 并登记（不跑 migrate，避免提前应用 0003+）
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _toogl_migrations (
                version INTEGER PRIMARY KEY,
                description TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        for m in &MIGRATIONS[..2] {
            apply_sql(&pool, m.sql).await;
            sqlx::query("INSERT INTO _toogl_migrations (version, description) VALUES (?, ?)")
                .bind(m.version)
                .bind(m.description)
                .execute(&pool)
                .await
                .unwrap();
        }

        // 旧表造数：两个文件、三条事件（事件 3 挂在文件 2 上）、两条日聚合
        sqlx::query(
            "INSERT INTO kimi_log_file (id, path, offset, size, mtime, last_scanned_at)
             VALUES (10, 'C:/logs/a/wire.jsonl', 100, 100, 111, '2026-01-01T00:00:00Z'),
                    (20, 'C:/logs/b/wire.jsonl', 200, 200, 222, '2026-01-02T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO kimi_usage_event
                 (log_file_id, ts, session_id, project, model, input_other, output, cache_read, cache_creation)
             VALUES (10, 1000, 's1', 'app', 'm1', 10, 1, 90, 0),
                    (10, 2000, 's1', 'app', 'm2', 20, 2, 0, 5),
                    (20, 3000, 's2', 'blog', 'm1', 30, 3, 60, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO kimi_usage_daily
                 (date, project, model, input_other, output, cache_read, cache_creation)
             VALUES ('2026-01-01', 'app', 'm1', 10, 1, 90, 0),
                    ('2026-01-01', 'app', 'm2', 20, 2, 0, 5)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 手动应用 0003 并登记（不走 migrate：0004 会 DROP kimi_* 旧表，
        // 之后的断言与 0003 重放都依赖旧表仍在）
        apply_sql(&pool, MIGRATIONS[2].sql).await;
        sqlx::query("INSERT INTO _toogl_migrations (version, description) VALUES (?, ?)")
            .bind(MIGRATIONS[2].version)
            .bind(MIGRATIONS[2].description)
            .execute(&pool)
            .await
            .unwrap();

        // 游标回填：platform='kimi'，字段原样保留；新 id 与旧 id 无关（经 path 映射）
        let files: Vec<(i64, String, String, i64, i64, i64)> = sqlx::query_as(
            "SELECT id, platform, path, offset, size, mtime FROM log_file ORDER BY path",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].1, "kimi");
        assert_eq!(files[0].2, "C:/logs/a/wire.jsonl");
        assert_eq!((files[0].3, files[0].4, files[0].5), (100, 100, 111));
        let new_id_a = files[0].0;
        let new_id_b = files[1].0;

        // 事件回填：log_file_id 映射到新 id，cli/cost 正确
        let events: Vec<(String, String, i64, i64, String, Option<f64>)> = sqlx::query_as(
            "SELECT platform, cli, log_file_id, ts, project, cost FROM usage_event ORDER BY ts",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].0, "kimi");
        assert_eq!(events[0].1, "kimi-code");
        assert_eq!(events[0].2, new_id_a);
        assert_eq!(events[2].2, new_id_b);
        assert!(events.iter().all(|e| e.5.is_none()));

        // 日聚合回填：platform/cli 列补齐，数值不变
        let daily: Vec<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT date, platform, cli, model, input_other FROM usage_daily ORDER BY model",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(daily.len(), 2);
        assert_eq!(daily[0].1, "kimi");
        assert_eq!(daily[0].2, "kimi-code");
        assert_eq!(daily[0].4, 10);
        assert_eq!(daily[1].4, 20);

        // 幂等：语句级重复执行 0003，数据不重不丢
        apply_sql(&pool, MIGRATIONS[2].sql).await;
        let (file_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM log_file")
            .fetch_one(&pool)
            .await
            .unwrap();
        let (event_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_event")
            .fetch_one(&pool)
            .await
            .unwrap();
        let (daily_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_daily")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((file_count, event_count, daily_count), (2, 3, 2));
        // 重复回填后事件仍挂在原 log_file id 上（log_file 未被 DELETE 重建）
        let (mapped,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM usage_event WHERE log_file_id IN (?, ?)")
                .bind(new_id_a)
                .bind(new_id_b)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(mapped, 3);
    }

    /// 空库直接全量迁移：回填语句在 kimi_* 空表上是安全的 no-op
    #[tokio::test]
    async fn migration_0003_backfill_noop_on_empty_old_tables() {
        let (_tmp, pool) = temp_pool().await;
        migrate(&pool).await.unwrap();
        let (event_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_event")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(event_count, 0);
    }

    /// 0006 清理：五张表的非 kimi 存量数据全部删除，kimi 行保留；重复迁移幂等
    #[tokio::test]
    async fn migration_0006_deletes_non_kimi_data() {
        let (_tmp, pool) = temp_pool().await;
        migrate(&pool).await.unwrap();

        // 造数：kimi 与 mimo/omp/deepseek 各一份
        sqlx::query(
            "INSERT INTO log_file (platform, path, offset, size, mtime, last_scanned_at)
             VALUES ('kimi', 'k.log', 0, 0, 0, '2026-01-01T00:00:00Z'),
                    ('omp', 'o.log', 0, 0, 0, '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO usage_event
                 (platform, cli, log_file_id, ts, session_id, project, model,
                  input_other, output, cache_read, cache_creation, cost)
             SELECT platform, 'cli', id, 1000, 's1', 'app', 'm1', 1, 1, 0, 0, NULL FROM log_file",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO usage_daily
                 (date, platform, cli, project, model, input_other, output, cache_read, cache_creation)
             VALUES ('2026-01-01', 'kimi', 'kimi-code', 'app', 'm1', 1, 1, 0, 0),
                    ('2026-01-01', 'mimo', 'mimocode', 'app', 'm1', 1, 1, 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO quota_snapshot (platform, remaining, fetched_at)
             VALUES ('kimi', 1.0, '2026-01-01T00:00:00Z'),
                    ('deepseek', 2.0, '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO platform_account (platform, name, enabled)
             VALUES ('kimi', '主账号', 1), ('deepseek', 'DS', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 删除已登记的 0006 记录并手动重放（migrate 已跑过一轮，重放验证幂等）
        sqlx::query("DELETE FROM _toogl_migrations WHERE version = 6")
            .execute(&pool)
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        // 再来一遍：已登记跳过 + 语句级幂等，数据不变
        apply_sql(&pool, MIGRATIONS[5].sql).await;

        for table in [
            "log_file",
            "usage_event",
            "usage_daily",
            "quota_snapshot",
            "platform_account",
        ] {
            let (total, kimi): (i64, i64) = sqlx::query_as(&format!(
                "SELECT COUNT(*), COALESCE(SUM(platform = 'kimi'), 0) FROM {table}"
            ))
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!((total, kimi), (1, 1), "表 {table} 应只剩 kimi 一行");
        }
    }

    /// 0007：usage_daily 增加 request_count 列并按事件表回填；列守卫保证重放幂等
    #[tokio::test]
    async fn migration_0007_adds_request_count_and_backfills() {
        let (_tmp, pool) = temp_pool().await;
        // 模拟 0006 时代的既有库：应用 0001~0006 并登记（0007 未应用，usage_daily 无 request_count）
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _toogl_migrations (
                version INTEGER PRIMARY KEY,
                description TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        for m in &MIGRATIONS[..6] {
            apply_sql(&pool, m.sql).await;
            sqlx::query("INSERT INTO _toogl_migrations (version, description) VALUES (?, ?)")
                .bind(m.version)
                .bind(m.description)
                .execute(&pool)
                .await
                .unwrap();
        }

        // 造数：两条事件（同日同项目同模型）+ 一条对应的日聚合（缺 request_count）
        sqlx::query(
            "INSERT INTO log_file (platform, path, offset, size, mtime, last_scanned_at)
             VALUES ('kimi', 'k.log', 0, 0, 0, '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO usage_event
                 (platform, cli, log_file_id, ts, session_id, project, model,
                  input_other, output, cache_read, cache_creation, cost)
             VALUES ('kimi', 'kimi-code', 1, 1000, 's1', 'app', 'm1', 10, 1, 0, 0, NULL),
                    ('kimi', 'kimi-code', 1, 2000, 's1', 'app', 'm1', 20, 2, 0, 0, NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO usage_daily
                 (date, platform, cli, project, model, input_other, output, cache_read, cache_creation)
             VALUES ('1970-01-01', 'kimi', 'kimi-code', 'app', 'm1', 30, 3, 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        migrate(&pool).await.unwrap();

        // 列已存在，回填后 request_count = 事件行数
        let (input, request_count): (i64, i64) =
            sqlx::query_as("SELECT input_other, request_count FROM usage_daily")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((input, request_count), (30, 2));

        // 重放幂等：删除登记记录再 migrate，列守卫跳过 ALTER，数据不变
        sqlx::query("DELETE FROM _toogl_migrations WHERE version = 7")
            .execute(&pool)
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        let (count, total): (i64, i64) =
            sqlx::query_as("SELECT COUNT(*), SUM(request_count) FROM usage_daily")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((count, total), (1, 2));
        let (registered,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM _toogl_migrations WHERE version = 7")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(registered, 1);
    }

    /// 0004 收尾：kimi_account 配置迁入 platform_account（credential_ref 保留，
    /// keyring 既有凭据不失效），旧表全部 DROP；重复执行幂等且不覆盖新配置
    #[tokio::test]
    async fn migration_0004_migrates_account_and_drops_legacy_tables() {
        let (_tmp, pool) = temp_pool().await;
        // 模拟既有数据库：应用 0001~0003 并登记
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _toogl_migrations (
                version INTEGER PRIMARY KEY,
                description TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        for m in &MIGRATIONS[..3] {
            apply_sql(&pool, m.sql).await;
            sqlx::query("INSERT INTO _toogl_migrations (version, description) VALUES (?, ?)")
                .bind(m.version)
                .bind(m.description)
                .execute(&pool)
                .await
                .unwrap();
        }
        // 旧账户表造数（用户已配置的 Kimi 开放平台账户）
        sqlx::query(
            "INSERT INTO kimi_account (id, name, api_base, credential_ref, enabled)
             VALUES (1, '主账号', 'https://api.moonshot.cn', 'kimi-openplatform-api-key', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        migrate(&pool).await.unwrap();

        // 配置已迁入 platform_account，credential_ref 原样保留
        let row: (String, String, String, String, i64) = sqlx::query_as(
            "SELECT platform, name, api_base, credential_ref, enabled FROM platform_account",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "kimi");
        assert_eq!(row.1, "主账号");
        assert_eq!(row.3, "kimi-openplatform-api-key");
        assert_eq!(row.4, 1);

        // 旧表全部 DROP
        for table in [
            "platform",
            "usage_snapshot",
            "kimi_account",
            "kimi_log_file",
            "kimi_usage_event",
            "kimi_usage_daily",
            "kimi_balance_snapshot",
        ] {
            let (ok,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE name = ?")
                .bind(table)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(ok, 0, "旧表 {table} 应已被 DROP");
        }

        // 幂等：再走一遍 migrate（0004 已登记跳过），配置行不重复不覆盖
        sqlx::query("UPDATE platform_account SET name = '改名后' WHERE platform = 'kimi'")
            .execute(&pool)
            .await
            .unwrap();
        migrate(&pool).await.unwrap();
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM platform_account WHERE platform = 'kimi'")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "改名后");
    }
}
