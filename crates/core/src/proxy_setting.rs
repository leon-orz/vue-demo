//! M5 代理模块设置的简单 KV 持久化（proxy_setting 表，migration 0005）。
//!
//! 当前记录 mihomo sidecar external-controller 实际端口与鉴权密钥（secret）、
//! mixed-port 实际端口（冲突顺延后的真实值）、记住的节点选择与规则模式；系统代理状态备份（sysproxy::ProxyBackupState
//! JSON 的 SQLite 版 ProxyStateStore）也基于本表实现（见 [`SqliteProxyStateStore`]）。
//! 与其他工具共用桌面端 toogl.db，本模块只管理代理配置和现场备份。

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

use crate::sysproxy::{ProxyBackupState, ProxyStateStore, SysProxyError};

/// sidecar external-controller 实际端口（值：十进制端口号字符串）
pub const KEY_CONTROLLER_PORT: &str = "mihomo.controller_port";
/// sidecar external-controller 鉴权密钥（值：随机 hex 串）。
/// 首跑生成配置模板时随机生成并同时写入 config.yaml 的 secret 字段与本表；
/// 已有配置缺 secret 且为本工具生成的模板时启动时自动补写（见 desktop 侧 sidecar.rs）
pub const KEY_CONTROLLER_SECRET: &str = "mihomo.controller_secret";
/// sidecar 本地混合代理（mixed-port）实际端口（值：十进制端口号字符串）。
/// 首跑生成配置时在 7890~7900 顺延探测；已有配置以 config.yaml 解析值为准，
/// sidecar 每次拉起都会刷新本 key
pub const KEY_MIXED_PORT: &str = "mihomo.mixed_port";
/// 系统代理状态备份（值：sysproxy::ProxyBackupState 的 JSON）
pub const KEY_PROXY_BACKUP: &str = "sysproxy.backup_state";
/// 记住的节点选择（值：models::ProxySelection 的 JSON；mihomo 侧才是事实源，仅兜底展示）
pub const KEY_SELECTION: &str = "proxy.selection";
/// 记住的规则模式（值：rule/global/direct 小写串；事实源为 mihomo /configs，仅降级兜底）
pub const KEY_RULE_MODE: &str = "proxy.rule_mode";
/// 订阅链接（值：URL 字符串；M5-07 节点导入，桌面端编排层读写）
pub const KEY_SUBSCRIPTION_URL: &str = "proxy.subscription_url";
/// 订阅节点列表（值：mihomo proxies 条目数组的 JSON 串，同 KEY_SELECTION 先例；core 只给键，序列化在桌面端）
pub const KEY_SUBSCRIPTION_NODES: &str = "proxy.subscription_nodes";
/// 订阅上次成功更新时间（值：RFC 3339 字符串）
pub const KEY_SUBSCRIPTION_LAST_UPDATED: &str = "proxy.subscription_last_updated";
/// 订阅上次更新失败原因（值：错误文案；成功时清除）
pub const KEY_SUBSCRIPTION_LAST_ERROR: &str = "proxy.subscription_last_error";
/// 手动粘贴导入的节点列表（值：mihomo proxies 条目数组的 JSON 串）
pub const KEY_MANUAL_NODES: &str = "proxy.manual_nodes";

/// 生成 external-controller 鉴权密钥：32 位随机 hex（UUID v4 去连字符）。
/// 纯本地随机，不回源；写入 mihomo 配置 secret 字段并持久化到 proxy_setting。
pub fn generate_controller_secret() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// proxy_setting 表读写器：持有一个 SQLite 连接池
#[derive(Debug, Clone)]
pub struct ProxySettingStore {
    pool: SqlitePool,
}

impl ProxySettingStore {
    /// 打开（必要时创建）数据库并执行 schema 迁移（幂等），WAL 模式。
    /// 与 mihomo sidecar 共用应用数据库，使用 WAL 和 busy_timeout 支持多连接。
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

    /// 删除 key（不存在为 no-op，幂等）
    pub async fn delete(&self, key: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM proxy_setting WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// sysproxy::ProxyStateStore 的 SQLite 实现：ProxyBackupState 序列化为 JSON
/// 存进 proxy_setting 表（KEY_PROXY_BACKUP），与系统代理备份 seam 对接。
///
/// 同步 trait 与异步 sqlx 的桥接：内置一个专用 current_thread 运行时跑 block_on。
/// 调用纪律：所有方法必须在**同步上下文**调用——`Runtime::block_on` 在已进入
/// runtime 的线程上会 panic（Cannot start a runtime from within a runtime）。
/// Tauri 命令层一律经 `async_runtime::spawn_blocking` 调用；RunEvent::Exit 是纯
/// 同步上下文可直接调用。**注意 setup 经 `tauri::async_runtime::block_on` 驱动异步
/// 初始化时处于 runtime 上下文，禁止在其中直接调用本类型（及含本类型的
/// SystemProxyManager）的同步方法**。
#[derive(Clone)]
pub struct SqliteProxyStateStore {
    settings: ProxySettingStore,
    rt: std::sync::Arc<NonBlockingRuntime>,
}

/// Runtime 包装：Drop 时用 shutdown_background 收尾——直接 drop Runtime 会阻塞等待
/// 阻塞线程池，在异步上下文里 drop（如测试收尾、State 释放）会 panic。
struct NonBlockingRuntime(Option<tokio::runtime::Runtime>);

impl NonBlockingRuntime {
    fn new() -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("proxy 状态存储运行时构建失败");
        Self(Some(rt))
    }

    fn block_on<F: std::future::Future>(&self, future: F) -> F::Output {
        self.0
            .as_ref()
            .expect("proxy 状态存储运行时已关闭")
            .block_on(future)
    }
}

impl Drop for NonBlockingRuntime {
    fn drop(&mut self) {
        if let Some(rt) = self.0.take() {
            rt.shutdown_background();
        }
    }
}

impl SqliteProxyStateStore {
    pub fn new(settings: ProxySettingStore) -> Self {
        Self {
            settings,
            rt: std::sync::Arc::new(NonBlockingRuntime::new()),
        }
    }
}

impl ProxyStateStore for SqliteProxyStateStore {
    fn load(&self) -> Result<Option<ProxyBackupState>, SysProxyError> {
        let json = self
            .rt
            .block_on(self.settings.get(KEY_PROXY_BACKUP))
            .map_err(|e| SysProxyError::Store(e.to_string()))?;
        json.map(|s| {
            serde_json::from_str(&s)
                .map_err(|e| SysProxyError::Store(format!("备份状态 JSON 解析失败: {e}")))
        })
        .transpose()
    }

    fn save(&self, state: &ProxyBackupState) -> Result<(), SysProxyError> {
        let json = serde_json::to_string(state)
            .map_err(|e| SysProxyError::Store(format!("备份状态序列化失败: {e}")))?;
        self.rt
            .block_on(self.settings.set(KEY_PROXY_BACKUP, &json))
            .map_err(|e| SysProxyError::Store(e.to_string()))
    }

    fn clear(&self) -> Result<(), SysProxyError> {
        self.rt
            .block_on(self.settings.delete(KEY_PROXY_BACKUP))
            .map_err(|e| SysProxyError::Store(e.to_string()))
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

    // ---- external-controller secret 生成与持久化 ----

    #[test]
    fn generate_controller_secret_is_random_hex() {
        let a = generate_controller_secret();
        let b = generate_controller_secret();
        // UUID v4 simple 形态：32 位小写 hex
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // 两次生成必然不同（随机性来源，碰撞概率可忽略）
        assert_ne!(a, b);
    }

    #[tokio::test]
    async fn controller_secret_persists_via_kv() {
        let (_tmp, store) = temp_store().await;
        let secret = generate_controller_secret();
        store.set(KEY_CONTROLLER_SECRET, &secret).await.unwrap();
        assert_eq!(
            store.get(KEY_CONTROLLER_SECRET).await.unwrap().as_deref(),
            Some(secret.as_str())
        );
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
        // 第二个连接（模拟 sidecar 与代理命令层各连一次同一库文件）
        let b = ProxySettingStore::connect(&path).await.unwrap();
        a.set(KEY_CONTROLLER_PORT, "9090").await.unwrap();
        assert_eq!(
            b.get(KEY_CONTROLLER_PORT).await.unwrap().as_deref(),
            Some("9090")
        );
    }

    #[tokio::test]
    async fn delete_removes_key_and_is_idempotent() {
        let (_tmp, store) = temp_store().await;
        store.set(KEY_SELECTION, "{}").await.unwrap();
        store.delete(KEY_SELECTION).await.unwrap();
        assert_eq!(store.get(KEY_SELECTION).await.unwrap(), None);
        // 重复删除不报错
        store.delete(KEY_SELECTION).await.unwrap();
    }

    // ---- SqliteProxyStateStore（sysproxy::ProxyStateStore 的 SQLite 实现） ----
    // 同步 trait 方法内部 block_on，异步测试里必须经 spawn_blocking 调用

    use crate::models::SystemProxyStatus;

    fn backup(enabled: bool, server: Option<&str>) -> ProxyBackupState {
        ProxyBackupState {
            previous: SystemProxyStatus {
                enabled,
                server: server.map(str::to_string),
            },
        }
    }

    #[tokio::test]
    async fn sqlite_state_store_save_load_clear_roundtrip() {
        let (_tmp, settings) = temp_store().await;
        let store = SqliteProxyStateStore::new(settings);

        let s = store.clone();
        let loaded = tokio::task::spawn_blocking(move || s.load().unwrap())
            .await
            .unwrap();
        assert_eq!(loaded, None);

        let s = store.clone();
        tokio::task::spawn_blocking(move || s.save(&backup(true, Some("corp:3128"))).unwrap())
            .await
            .unwrap();
        let s = store.clone();
        let loaded = tokio::task::spawn_blocking(move || s.load().unwrap())
            .await
            .unwrap();
        assert_eq!(loaded, Some(backup(true, Some("corp:3128"))));

        // 覆盖保存（重复开启不覆盖由 SystemProxyManager 保证，这里只验证存取本身幂等）
        let s = store.clone();
        tokio::task::spawn_blocking(move || s.save(&backup(false, None)).unwrap())
            .await
            .unwrap();
        let s = store.clone();
        tokio::task::spawn_blocking(move || s.clear().unwrap())
            .await
            .unwrap();
        let s = store.clone();
        let loaded = tokio::task::spawn_blocking(move || s.load().unwrap())
            .await
            .unwrap();
        assert_eq!(loaded, None);
    }

    #[tokio::test]
    async fn sqlite_state_store_corrupt_json_is_store_error() {
        let (_tmp, settings) = temp_store().await;
        settings.set(KEY_PROXY_BACKUP, "not-json").await.unwrap();
        let store = SqliteProxyStateStore::new(settings);
        let err = tokio::task::spawn_blocking(move || store.load().unwrap_err())
            .await
            .unwrap();
        assert!(matches!(err, SysProxyError::Store(_)));
    }
}
