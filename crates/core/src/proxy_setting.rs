//! M5 代理模块设置的简单 KV 持久化（proxy_setting 表，migration 0005）。
//!
//! 当前记录 mihomo sidecar external-controller 实际端口（9090 冲突顺延后的真实值）；
//! 项3系统代理状态备份（sysproxy::ProxyBackupState JSON 的 SQLite 版 ProxyStateStore）
//! 也基于本表实现。桌面端与 hub 各自连自己的库文件，本模块只做纯 KV 读写。

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

/// sidecar external-controller 实际端口（值：十进制端口号字符串）
pub const KEY_CONTROLLER_PORT: &str = "mihomo.controller_port";
/// 系统代理状态备份（项3用，值：sysproxy::ProxyBackupState 的 JSON）
pub const KEY_PROXY_BACKUP: &str = "sysproxy.backup_state";

/// proxy_setting 表读写器：持有一个 SQLite 连接池
#[derive(Debug, Clone)]
pub struct ProxySettingStore {
    pool: SqlitePool,
}

impl ProxySettingStore {
    /// 打开（必要时创建）数据库并执行 schema 迁移（幂等），WAL 模式。
    /// 与 UsageService 连同一库文件也安全（WAL + busy_timeout 多连接共存）。
    pub async fn connect(db_path: impl AsRef<Path>) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(db_path.as_ref())
            .create_if_missing(true)
            .pragma("journal_mode", "WAL")
            .pragma("busy_timeout", "5000");
        let pool = SqlitePool::connect_with(options).await?;
        crate::db::migrate(&pool).await?;
        Ok(Self { pool })
    }

    /// 复用已有连接池（调用方保证迁移已执行）
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// 无记录返回 Ok(None)
    pub async fn get(&self, key: &str) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM proxy_setting WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(v,)| v))
    }

    /// 幂等 upsert（INSERT OR REPLACE）
    pub async fn set(&self, key: &str, value: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO proxy_setting (key, value, updated_at)
             VALUES (?, ?, datetime('now'))",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn temp_store() -> (tempfile::TempDir, ProxySettingStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProxySettingStore::connect(tmp.path().join("t.db"))
            .await
            .unwrap();
        (tmp, store)
    }

    #[tokio::test]
    async fn get_returns_none_when_missing() {
        let (_tmp, store) = temp_store().await;
        assert_eq!(store.get(KEY_CONTROLLER_PORT).await.unwrap(), None);
    }

    #[tokio::test]
    async fn set_get_roundtrip_and_overwrite() {
        let (_tmp, store) = temp_store().await;
        store.set(KEY_CONTROLLER_PORT, "9090").await.unwrap();
        assert_eq!(
            store.get(KEY_CONTROLLER_PORT).await.unwrap().as_deref(),
            Some("9090")
        );
        // 顺延场景：覆盖为实际端口
        store.set(KEY_CONTROLLER_PORT, "9093").await.unwrap();
        assert_eq!(
            store.get(KEY_CONTROLLER_PORT).await.unwrap().as_deref(),
            Some("9093")
        );
        // 不同 key 互不影响
        store.set(KEY_PROXY_BACKUP, "{}").await.unwrap();
        assert_eq!(
            store.get(KEY_CONTROLLER_PORT).await.unwrap().as_deref(),
            Some("9093")
        );
        assert_eq!(
            store.get(KEY_PROXY_BACKUP).await.unwrap().as_deref(),
            Some("{}")
        );
    }

    #[tokio::test]
    async fn connect_twice_on_same_db_is_safe() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.db");
        let a = ProxySettingStore::connect(&path).await.unwrap();
        // 第二个连接（模拟 UsageService 与 sidecar 各连一次同一库文件）
        let b = ProxySettingStore::connect(&path).await.unwrap();
        a.set(KEY_CONTROLLER_PORT, "9090").await.unwrap();
        assert_eq!(
            b.get(KEY_CONTROLLER_PORT).await.unwrap().as_deref(),
            Some("9090")
        );
    }
}
