//! AI 用量统计命令（M1）：薄壳，业务逻辑在 toogl_core::usage。
//! 2026-10-02 决策：当前只统计 Kimi Code，platform 参数自然只剩 kimi 可用
//! （其他平台待回归时从 git 历史恢复）；命令集合与签名不动。
//! 前端对应封装见 apps/desktop/src/api/usage.ts。
//! 所有命令统一返回 Result<T, CommandError>，前端 invoke 抛出的即 CommandError JSON。

use std::path::PathBuf;

use tauri::State;
use toogl_core::keystore::{KeyringStore, KEYRING_SERVICE_NAME};
use toogl_core::models::{
    CommandError, Platform, PlatformAccountInfo, PlatformAccountInput, PlatformFilter,
    PlatformScanReport, QuotaSnapshot, UsageGroup, UsageRange, UsageSeries, UsageSummary,
};
use toogl_core::usage::UsageService;

/// M1 模块共享状态。启动初始化失败时降级为错误态（unavailable），
/// 应用其余模块照常可用，本模块命令统一返回可读错误。
pub struct UsageState {
    inner: Result<UsageService, String>,
}

impl UsageState {
    /// 应用启动时构造：SQLite 落在 app data 目录下的 toogl.db，并执行 schema 迁移
    pub async fn new(app_data_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
        let service = UsageService::connect(app_data_dir.join("toogl.db"))
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self { inner: Ok(service) })
    }

    /// 构造降级错误态（初始化失败原因）
    pub fn unavailable(reason: String) -> Self {
        Self { inner: Err(reason) }
    }

    fn require(&self) -> Result<&UsageService, CommandError> {
        self.inner
            .as_ref()
            .map_err(|e| CommandError::unavailable(format!("用量模块初始化失败: {e}")))
    }
}

fn keyring() -> KeyringStore {
    KeyringStore::new(KEYRING_SERVICE_NAME)
}

/// 手动触发增量扫描（入口为前端用量页初始化与「重新扫描」按钮；各 store 内互斥锁防并发双计）。
/// platform 不传 = 全部有日志的平台（当前只有 kimi）；
/// 单平台失败不影响其他平台，各自结果/错误体现在返回值的每个元素里。
/// sessions_dir 为 Kimi 日志目录覆盖（M6 设置项），只作用于 kimi 平台。
#[tauri::command]
pub async fn scan_usage_logs(
    state: State<'_, UsageState>,
    platform: Option<Platform>,
    sessions_dir: Option<String>,
) -> Result<Vec<PlatformScanReport>, CommandError> {
    let service = state.require()?;
    Ok(service
        .scan(platform, sessions_dir.map(PathBuf::from).as_deref())
        .await)
}

/// 聚合用量总览（天 / 周 / 月 / 最近 5 小时）；platform='all' 为跨平台合计
#[tauri::command]
pub async fn usage_summary(
    state: State<'_, UsageState>,
    platform: PlatformFilter,
    range: UsageRange,
) -> Result<UsageSummary, CommandError> {
    state.require()?.summary(platform, range).await
}

#[tauri::command]
pub async fn usage_by_project(
    state: State<'_, UsageState>,
    platform: PlatformFilter,
    range: UsageRange,
) -> Result<Vec<UsageGroup>, CommandError> {
    state.require()?.usage_by_project(platform, range).await
}

#[tauri::command]
pub async fn usage_by_model(
    state: State<'_, UsageState>,
    platform: PlatformFilter,
    range: UsageRange,
) -> Result<Vec<UsageGroup>, CommandError> {
    state.require()?.usage_by_model(platform, range).await
}

/// 用量趋势时间序列（Day 按小时、Week/Month 按日），供趋势图使用
#[tauri::command]
pub async fn usage_series(
    state: State<'_, UsageState>,
    platform: PlatformFilter,
    range: UsageRange,
) -> Result<UsageSeries, CommandError> {
    state.require()?.usage_series(platform, range).await
}

/// 拉取/重算平台配额并写快照（编排在 usage::quota）
#[tauri::command]
pub async fn quota_refresh(
    state: State<'_, UsageState>,
    platform: Platform,
) -> Result<QuotaSnapshot, CommandError> {
    state.require()?.quota_refresh(platform, &keyring()).await
}

/// 读取最近一次配额快照（不触发网络请求，页面打开时先展示缓存）
#[tauri::command]
pub async fn quota_latest(
    state: State<'_, UsageState>,
    platform: Platform,
) -> Result<Option<QuotaSnapshot>, CommandError> {
    state.require()?.quota_latest(platform).await
}

/// 读取平台账户配置（不含凭据本体，只有 has_credential 标记）
#[tauri::command]
pub async fn platform_account_get(
    state: State<'_, UsageState>,
    platform: Platform,
) -> Result<Option<PlatformAccountInfo>, CommandError> {
    state.require()?.account_get(platform, &keyring()).await
}

/// 保存平台账户配置；api_key 非空时写入 keyring（不落库），None 表示沿用已有凭据。
/// 以 platform 参数为准（input.platform 为冗余自描述）。
#[tauri::command]
pub async fn platform_account_set(
    state: State<'_, UsageState>,
    platform: Platform,
    input: PlatformAccountInput,
    api_key: Option<String>,
) -> Result<(), CommandError> {
    state
        .require()?
        .account_set(platform, &input, api_key.as_deref(), &keyring())
        .await
}
