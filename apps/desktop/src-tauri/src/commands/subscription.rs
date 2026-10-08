//! M5-07 节点导入命令层（薄壳）：订阅状态查询 / 设置订阅链接 / 手动更新 /
//! 粘贴分享链接导入，以及 lib.rs setup 拉起的 24h 定时更新循环。
//! 解析、拉取、托管文件、接线与更新/导入编排在 toogl_core::subscription，
//! 本文件只做 State 取用与定时任务调度。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::State;
use toogl_core::mihomo::MihomoClient;
use toogl_core::models::{CommandError, ImportResult, SubscriptionInfo};
use toogl_core::proxy_setting::{
    ProxySettingStore, KEY_CONTROLLER_PORT, KEY_CONTROLLER_SECRET, KEY_SUBSCRIPTION_LAST_ERROR,
    KEY_SUBSCRIPTION_LAST_UPDATED, KEY_SUBSCRIPTION_URL,
};
use toogl_core::subscription::{import_merge, read_status, run_update, validate_subscription_url};

use crate::sidecar::ProxyCoreState;

/// 订阅定时更新间隔（24h），机场包月订阅的典型更新频率
pub const SUBSCRIPTION_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 3600);

/// 订阅命令层共享状态。设置库打不开时降级（settings 为 None），
/// 四条命令统一返回 unavailable，不影响其余模块。
pub struct SubscriptionCommandState {
    /// proxy_setting KV（订阅 URL / 节点列表 / 元信息）
    settings: Option<ProxySettingStore>,
    /// mihomo 工作目录（与 sidecar.rs 一致：app_data_dir/mihomo）
    work_dir: PathBuf,
    /// config.yaml 路径（ensure_wired 接线目标）
    config_path: PathBuf,
    /// 更新互斥标志：命令路径与定时任务路径共享，拒绝并发（compare_exchange 抢占）。
    /// 选 AtomicBool 而非 tokio Mutex：并发语义是「直接拒绝」而非排队，
    /// 原子标志即可表达，且定时任务/命令两侧都无需持锁跨 await。
    updating: Arc<AtomicBool>,
    /// 初始化失败原因（降级态）
    init_error: Option<String>,
}

/// run_update / import_merge 的调用上下文（Clone）：命令路径从 State 取，
/// 定时任务路径在 setup 时自持一份（见 lib.rs）。
#[derive(Clone)]
pub struct SubscriptionUpdater {
    pub store: ProxySettingStore,
    pub work_dir: PathBuf,
    pub config_path: PathBuf,
    pub updating: Arc<AtomicBool>,
}

impl SubscriptionCommandState {
    /// 应用 setup 时构造（经 tauri::async_runtime::block_on 驱动）：打开 proxy_setting
    /// 库（与 proxy 命令层同一 toogl.db）。失败收敛为降级态，不 panic。
    pub async fn new(app_data_dir: PathBuf) -> Self {
        let work_dir = app_data_dir.join("mihomo");
        let config_path = work_dir.join("config.yaml");
        match ProxySettingStore::connect(app_data_dir.join("toogl.db")).await {
            Ok(settings) => Self {
                settings: Some(settings),
                work_dir,
                config_path,
                updating: Arc::new(AtomicBool::new(false)),
                init_error: None,
            },
            Err(e) => Self::degraded(format!("订阅设置库打开失败: {e}"), work_dir, config_path),
        }
    }

    /// 降级态（app data 目录解析失败等；work_dir 无意义置空，命令全部 unavailable）
    pub fn unavailable(reason: String) -> Self {
        Self::degraded(reason, PathBuf::new(), PathBuf::new())
    }

    fn degraded(reason: String, work_dir: PathBuf, config_path: PathBuf) -> Self {
        Self {
            settings: None,
            work_dir,
            config_path,
            updating: Arc::new(AtomicBool::new(false)),
            init_error: Some(reason),
        }
    }

    fn store(&self) -> Result<&ProxySettingStore, CommandError> {
        self.settings.as_ref().ok_or_else(|| {
            CommandError::unavailable(format!(
                "订阅模块初始化失败: {}",
                self.init_error.as_deref().unwrap_or("未知原因")
            ))
        })
    }

    /// 定时任务自持的上下文（设置库降级时为 None，lib.rs 据此不拉起定时任务）
    pub fn updater(&self) -> Option<SubscriptionUpdater> {
        self.settings.clone().map(|store| SubscriptionUpdater {
            store,
            work_dir: self.work_dir.clone(),
            config_path: self.config_path.clone(),
            updating: Arc::clone(&self.updating),
        })
    }
}

// ---- 命令 ----

/// 订阅状态查询：URL / 上次成功时间 / 节点数 / 手动节点数 / 更新中标志 / 上次错误。
/// 未配置订阅是正常态（url 为 None），不报错。
#[tauri::command]
pub async fn subscription_status(
    state: State<'_, SubscriptionCommandState>,
) -> Result<SubscriptionInfo, CommandError> {
    let updating = state.updating.load(Ordering::SeqCst);
    read_status(state.store()?, updating).await
}

/// 设置订阅链接并立即拉取一次。URL 先过 core 校验（仅 http/https）；
/// 拉取失败时 URL 已保存（用户修正网络后可直接点更新），错误向上抛。
#[tauri::command]
pub async fn subscription_set_url(
    state: State<'_, SubscriptionCommandState>,
    core: State<'_, ProxyCoreState>,
    url: String,
) -> Result<SubscriptionInfo, CommandError> {
    let normalized = validate_subscription_url(&url).map_err(CommandError::from)?;
    let store = state.store()?;
    store.set(KEY_SUBSCRIPTION_URL, &normalized).await?;
    run_update(
        store,
        &state.work_dir,
        &state.config_path,
        core.client()?,
        &state.updating,
    )
    .await
}

/// 手动触发订阅更新（前端「更新订阅」按钮）
#[tauri::command]
pub async fn subscription_update(
    state: State<'_, SubscriptionCommandState>,
    core: State<'_, ProxyCoreState>,
) -> Result<SubscriptionInfo, CommandError> {
    let store = state.store()?;
    run_update(
        store,
        &state.work_dir,
        &state.config_path,
        core.client()?,
        &state.updating,
    )
    .await
}

/// 粘贴分享链接/订阅内容导入为手动节点（逐条容错：好的导入、坏的记入 failures）。
/// 同名手动节点被新粘贴的覆盖；与订阅节点并集时手动优先。
#[tauri::command]
pub async fn import_share_text(
    state: State<'_, SubscriptionCommandState>,
    core: State<'_, ProxyCoreState>,
    text: String,
) -> Result<ImportResult, CommandError> {
    let store = state.store()?;
    import_merge(
        store,
        &state.work_dir,
        &state.config_path,
        core.client()?,
        &state.updating,
        &text,
    )
    .await
}

// ---- 定时更新（lib.rs setup spawn） ----

/// 订阅定时更新循环：启动时按需补一次（有 URL 且超期/从未成功），之后每 24h 一次。
/// 任何失败只写 last_error + 日志，循环永不退出。
pub async fn subscription_refresh_loop(updater: SubscriptionUpdater) {
    if should_refresh(&updater.store).await {
        refresh_once(&updater).await;
    }
    let mut ticker = tokio::time::interval(SUBSCRIPTION_REFRESH_INTERVAL);
    // interval 首次立即到期，跳过（启动检查已覆盖本次）
    ticker.tick().await;
    loop {
        ticker.tick().await;
        refresh_once(&updater).await;
    }
}

/// 单次定时更新：sidecar 未运行（端口未知或探测不活）时跳过本次，
/// 记 last_error 提示，不作为任务错误。
async fn refresh_once(updater: &SubscriptionUpdater) {
    let Some(client) = controller_client(&updater.store).await else {
        set_last_error(
            &updater.store,
            "mihomo sidecar 未运行（端口未知），跳过本次订阅更新",
        )
        .await;
        return;
    };
    if !client.alive().await {
        set_last_error(&updater.store, "mihomo sidecar 未运行，跳过本次订阅更新").await;
        return;
    }
    // run_update 内部已写 last_error，这里只记日志
    if let Err(e) = run_update(
        &updater.store,
        &updater.work_dir,
        &updater.config_path,
        &client,
        &updater.updating,
    )
    .await
    {
        eprintln!("[subscription] 定时更新失败: {e}");
    }
}

/// 从 proxy_setting 读 external-controller 端口/secret 构造 mihomo 客户端
/// （与 sidecar.rs 拉起后构造同式；定时任务路径拿不到 ProxyCoreState，从 KV 自取）
async fn controller_client(store: &ProxySettingStore) -> Option<MihomoClient> {
    let port: u16 = store
        .get(KEY_CONTROLLER_PORT)
        .await
        .ok()
        .flatten()?
        .parse()
        .ok()?;
    let secret = store.get(KEY_CONTROLLER_SECRET).await.ok().flatten();
    Some(MihomoClient::new(
        format!("http://127.0.0.1:{port}"),
        secret,
    ))
}

/// 是否需要补一次更新：配置了 URL 且（无成功记录或距今超过一个刷新周期）
async fn should_refresh(store: &ProxySettingStore) -> bool {
    let Ok(Some(url)) = store.get(KEY_SUBSCRIPTION_URL).await else {
        return false;
    };
    if url.trim().is_empty() {
        return false;
    }
    let last = store
        .get(KEY_SUBSCRIPTION_LAST_UPDATED)
        .await
        .ok()
        .flatten();
    // 超期阈值从刷新间隔常量换算，消除「24h」双写
    let stale_after = chrono::Duration::from_std(SUBSCRIPTION_REFRESH_INTERVAL)
        .unwrap_or_else(|_| chrono::Duration::hours(24));
    match last
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
    {
        None => true,
        Some(t) => {
            chrono::Utc::now().signed_duration_since(t.with_timezone(&chrono::Utc)) > stale_after
        }
    }
}

async fn set_last_error(store: &ProxySettingStore, msg: &str) {
    if let Err(e) = store.set(KEY_SUBSCRIPTION_LAST_ERROR, msg).await {
        eprintln!("[subscription] 写入 last_error 失败: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试夹具：临时目录 + 真实 ProxySettingStore（tempfile db）
    async fn fixture() -> (tempfile::TempDir, ProxySettingStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProxySettingStore::connect(tmp.path().join("toogl.db"))
            .await
            .unwrap();
        (tmp, store)
    }

    #[tokio::test]
    async fn should_refresh_logic() {
        let (_tmp, store) = fixture().await;
        // 无 URL → 不刷
        assert!(!should_refresh(&store).await);
        store
            .set(KEY_SUBSCRIPTION_URL, "https://sub.example.com/f")
            .await
            .unwrap();
        // 有 URL 无成功记录 → 刷
        assert!(should_refresh(&store).await);
        // 刚更新过 → 不刷
        store
            .set(KEY_SUBSCRIPTION_LAST_UPDATED, &now_rfc3339())
            .await
            .unwrap();
        assert!(!should_refresh(&store).await);
        // 超期（刷新间隔 + 1h 前）→ 刷
        let stale = (chrono::Utc::now() - chrono::Duration::hours(25))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        store
            .set(KEY_SUBSCRIPTION_LAST_UPDATED, &stale)
            .await
            .unwrap();
        assert!(should_refresh(&store).await);
    }

    #[tokio::test]
    async fn controller_client_builds_from_kv() {
        let (_tmp, store) = fixture().await;
        // 无端口记录 → None
        assert!(controller_client(&store).await.is_none());
        store.set(KEY_CONTROLLER_PORT, "9095").await.unwrap();
        store.set(KEY_CONTROLLER_SECRET, "s3cret").await.unwrap();
        let client = controller_client(&store).await.unwrap();
        assert_eq!(client.base_url(), "http://127.0.0.1:9095");
    }

    /// 上次成功更新时间格式：RFC3339 UTC 秒级（与 core update 模块落盘格式一致）
    fn now_rfc3339() -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
}
