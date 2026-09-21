//! Kimi 统计命令（M1）：薄壳，业务逻辑在 toogl_core 的 kimicode / adapters::kimi。
//! 前端对应封装见 apps/desktop/src/api/kimi.ts。

use std::path::PathBuf;

use tauri::State;
use toogl_core::adapters::kimi::{KimiAdapter, DEFAULT_API_BASE};
use toogl_core::keystore::{KeyringStore, SecretStore};
use toogl_core::kimicode::{self, KimiStore};
use toogl_core::models::{
    KimiAccountInfo, KimiAccountInput, KimiBalance, KimiScanReport, KimiUsageGroup, KimiUsageRange,
    KimiUsageSeries, KimiUsageSummary,
};

use super::keyring::SERVICE_NAME;

/// keyring 中 Kimi 开放平台 key 的引用名（kimi_account.credential_ref 的默认值）
pub const KIMI_CREDENTIAL_REF: &str = "kimi-openplatform-api-key";

/// Kimi 模块共享状态：SQLite 连接池 + 会话日志目录（默认自动发现）
pub struct KimiState {
    pub store: KimiStore,
    pub sessions_dir: PathBuf,
}

impl KimiState {
    /// 应用启动时构造：db 与 tauri-plugin-sql 共用 app data 目录下的 toogl.db
    pub async fn new(app_data_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
        let store = KimiStore::connect(app_data_dir.join("toogl.db"))
            .await
            .map_err(|e| e.to_string())?;
        // 兜底建表：tauri-plugin-sql 的 migration 只在前端打开插件连接时才执行，
        // 前端不经插件直连数据库，表结构由这里保证（SCHEMA_SQL 与 0002_kimi.sql 同源）
        store.migrate().await.map_err(|e| e.to_string())?;
        let sessions_dir = kimicode::default_sessions_dir().map_err(|e| e.to_string())?;
        Ok(Self {
            store,
            sessions_dir,
        })
    }
}

fn keyring() -> KeyringStore {
    KeyringStore::new(SERVICE_NAME)
}

/// 手动触发增量扫描（启动时与定时任务复用同一逻辑）。
/// sessions_dir 留空则用默认目录（KIMI_CODE_HOME 或 ~/.kimi-code/sessions，M6 可覆盖）。
#[tauri::command]
pub async fn scan_kimi_logs(
    state: State<'_, KimiState>,
    sessions_dir: Option<String>,
) -> Result<KimiScanReport, String> {
    let dir = sessions_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| state.sessions_dir.clone());
    state
        .store
        .scan_and_record(&dir)
        .await
        .map_err(|e| e.to_string())
}

/// 聚合用量查询（天 / 周 / 月 / 最近 5 小时）
#[tauri::command]
pub async fn kimi_usage_summary(
    state: State<'_, KimiState>,
    range: KimiUsageRange,
) -> Result<KimiUsageSummary, String> {
    state.store.summary(range).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kimi_usage_by_project(
    state: State<'_, KimiState>,
    range: KimiUsageRange,
) -> Result<Vec<KimiUsageGroup>, String> {
    state
        .store
        .usage_by_project(range)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kimi_usage_by_model(
    state: State<'_, KimiState>,
    range: KimiUsageRange,
) -> Result<Vec<KimiUsageGroup>, String> {
    state
        .store
        .usage_by_model(range)
        .await
        .map_err(|e| e.to_string())
}

/// 用量趋势时间序列（Day 按小时、Week/Month 按日），供趋势图使用
#[tauri::command]
pub async fn kimi_usage_series(
    state: State<'_, KimiState>,
    range: KimiUsageRange,
) -> Result<KimiUsageSeries, String> {
    state
        .store
        .usage_series(range)
        .await
        .map_err(|e| e.to_string())
}

/// 读取最近一次余额快照（不触发网络请求，页面打开时先展示缓存）
#[tauri::command]
pub async fn kimi_balance_latest(
    state: State<'_, KimiState>,
) -> Result<Option<KimiBalance>, String> {
    state
        .store
        .latest_balance()
        .await
        .map_err(|e| e.to_string())
}

/// 拉取开放平台余额并写快照。账户未配置或 key 缺失时返回错误文案。
#[tauri::command]
pub async fn kimi_balance_refresh(state: State<'_, KimiState>) -> Result<KimiBalance, String> {
    let account = state
        .store
        .account_get()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "尚未配置 Kimi 开放平台账户".to_string())?;
    if !account.enabled {
        return Err("Kimi 开放平台账户已停用".to_string());
    }
    let key = keyring()
        .get(&account.credential_ref)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "keyring 中未找到 Kimi API key，请先在设置中配置".to_string())?;
    let balance = KimiAdapter::new()
        .fetch_balance(&account.api_base, &key)
        .await
        .map_err(|e| e.to_string())?;
    state
        .store
        .save_balance(&balance)
        .await
        .map_err(|e| e.to_string())?;
    Ok(balance)
}

/// 读取账户配置（不含凭据本体，只有 has_credential 标记）
#[tauri::command]
pub async fn kimi_account_get(
    state: State<'_, KimiState>,
) -> Result<Option<KimiAccountInfo>, String> {
    let Some(row) = state.store.account_get().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let has_credential = keyring()
        .get(&row.credential_ref)
        .map_err(|e| e.to_string())?
        .is_some();
    Ok(Some(KimiAccountInfo {
        name: row.name,
        api_base: row.api_base,
        credential_ref: row.credential_ref,
        has_credential,
        enabled: row.enabled,
    }))
}

/// 保存账户配置；api_key 非空时写入 keyring（不落库），None 表示沿用已有 key
#[tauri::command]
pub async fn kimi_account_set(
    state: State<'_, KimiState>,
    account: KimiAccountInput,
    api_key: Option<String>,
) -> Result<(), String> {
    state
        .store
        .account_set(
            &account.name,
            if account.api_base.is_empty() {
                DEFAULT_API_BASE
            } else {
                &account.api_base
            },
            KIMI_CREDENTIAL_REF,
            account.enabled,
        )
        .await
        .map_err(|e| e.to_string())?;
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        keyring()
            .set(KIMI_CREDENTIAL_REF, &key)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
