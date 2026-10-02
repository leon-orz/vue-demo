//! Kimi 统计命令（M1）：薄壳，业务逻辑在 toogl_core 的 kimicode / adapters::kimi。
//! 前端对应封装见 apps/desktop/src/api/kimi.ts。
//! 所有命令统一返回 Result<T, CommandError>，前端 invoke 抛出的即 CommandError JSON。

use std::path::PathBuf;

use tauri::State;
use toogl_core::adapters::kimi::DEFAULT_API_BASE;
use toogl_core::keystore::{KeyringStore, SecretStore, KEYRING_SERVICE_NAME};
use toogl_core::kimicode::{self, KimiStore, KIMI_CREDENTIAL_REF};
use toogl_core::models::{
    CommandError, KimiAccountInfo, KimiAccountInput, KimiBalance, ScanReport, UsageGroup,
    UsageRange, UsageSeries, UsageSummary,
};

/// Kimi 模块内部状态：SQLite 连接池 + 会话日志目录（默认自动发现）
struct KimiStateInner {
    store: KimiStore,
    sessions_dir: PathBuf,
}

/// Kimi 模块共享状态。启动初始化失败时降级为错误态（unavailable），
/// 应用其余模块照常可用，本模块命令统一返回可读错误。
pub struct KimiState {
    inner: Result<KimiStateInner, String>,
}

impl KimiState {
    /// 应用启动时构造：SQLite 落在 app data 目录下的 toogl.db，并执行 schema 迁移
    pub async fn new(app_data_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
        let store = KimiStore::connect(app_data_dir.join("toogl.db"))
            .await
            .map_err(|e| e.to_string())?;
        store.migrate().await.map_err(|e| e.to_string())?;
        let sessions_dir = kimicode::default_sessions_dir().map_err(|e| e.to_string())?;
        Ok(Self {
            inner: Ok(KimiStateInner {
                store,
                sessions_dir,
            }),
        })
    }

    /// 构造降级错误态（初始化失败原因）
    pub fn unavailable(reason: String) -> Self {
        Self { inner: Err(reason) }
    }

    fn require(&self) -> Result<&KimiStateInner, CommandError> {
        self.inner
            .as_ref()
            .map_err(|e| CommandError::unavailable(format!("Kimi 模块初始化失败: {e}")))
    }
}

fn keyring() -> KeyringStore {
    KeyringStore::new(KEYRING_SERVICE_NAME)
}

/// 手动触发扫描（启动时与定时任务复用同一逻辑；并发调用由 store 内互斥锁排队）。
/// sessions_dir 留空则用默认目录（KIMI_CODE_HOME 或 ~/.kimi-code/sessions，M6 可覆盖）。
#[tauri::command]
pub async fn scan_kimi_logs(
    state: State<'_, KimiState>,
    sessions_dir: Option<String>,
) -> Result<ScanReport, CommandError> {
    let inner = state.require()?;
    let dir = sessions_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| inner.sessions_dir.clone());
    Ok(inner.store.scan_and_record(&dir).await?)
}

/// 聚合用量查询（天 / 周 / 月 / 最近 5 小时）
#[tauri::command]
pub async fn kimi_usage_summary(
    state: State<'_, KimiState>,
    range: UsageRange,
) -> Result<UsageSummary, CommandError> {
    Ok(state.require()?.store.summary(range).await?)
}

#[tauri::command]
pub async fn kimi_usage_by_project(
    state: State<'_, KimiState>,
    range: UsageRange,
) -> Result<Vec<UsageGroup>, CommandError> {
    Ok(state.require()?.store.usage_by_project(range).await?)
}

#[tauri::command]
pub async fn kimi_usage_by_model(
    state: State<'_, KimiState>,
    range: UsageRange,
) -> Result<Vec<UsageGroup>, CommandError> {
    Ok(state.require()?.store.usage_by_model(range).await?)
}

/// 用量趋势时间序列（Day 按小时、Week/Month 按日），供趋势图使用
#[tauri::command]
pub async fn kimi_usage_series(
    state: State<'_, KimiState>,
    range: UsageRange,
) -> Result<UsageSeries, CommandError> {
    Ok(state.require()?.store.usage_series(range).await?)
}

/// 读取最近一次余额快照（不触发网络请求，页面打开时先展示缓存）
#[tauri::command]
pub async fn kimi_balance_latest(
    state: State<'_, KimiState>,
) -> Result<Option<KimiBalance>, CommandError> {
    Ok(state.require()?.store.latest_balance().await?)
}

/// 拉取开放平台余额并写快照（业务编排在 kimicode::refresh_balance）
#[tauri::command]
pub async fn kimi_balance_refresh(
    state: State<'_, KimiState>,
) -> Result<KimiBalance, CommandError> {
    kimicode::refresh_balance(&state.require()?.store, &keyring()).await
}

/// 读取账户配置（不含凭据本体，只有 has_credential 标记）
#[tauri::command]
pub async fn kimi_account_get(
    state: State<'_, KimiState>,
) -> Result<Option<KimiAccountInfo>, CommandError> {
    let Some(row) = state.require()?.store.account_get().await? else {
        return Ok(None);
    };
    let has_credential = keyring().get(&row.credential_ref)?.is_some();
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
) -> Result<(), CommandError> {
    state
        .require()?
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
        .await?;
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        keyring().set(KIMI_CREDENTIAL_REF, &key)?;
    }
    Ok(())
}
