//! M5-07 节点导入命令层（薄壳 + 编排）：订阅状态查询 / 设置订阅链接 / 手动更新 /
//! 粘贴分享链接导入，以及 lib.rs setup 拉起的 24h 定时更新循环。
//! 解析、拉取、托管文件与接线逻辑在 toogl_core::subscription，本文件只做编排：
//! 拉取 → 解析 → 与手动节点并集 → 写 toogl-nodes.yaml → ensure_wired 接线 →
//! mihomo 热重载 → 落盘订阅元信息（proxy_setting）。
//!
//! 编排主体收敛在自由函数 [`run_update`] / [`import_merge`]（参数为具体值而非
//! tauri State），使命令路径与定时任务路径共用同一实现，也便于单测。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_yaml::{Mapping, Value};
use tauri::State;
use toogl_core::mihomo::MihomoClient;
use toogl_core::models::{CommandError, ImportResult, SubscriptionInfo};
use toogl_core::proxy_setting::{
    ProxySettingStore, KEY_CONTROLLER_PORT, KEY_CONTROLLER_SECRET, KEY_MANUAL_NODES,
    KEY_SUBSCRIPTION_LAST_ERROR, KEY_SUBSCRIPTION_LAST_UPDATED, KEY_SUBSCRIPTION_NODES,
    KEY_SUBSCRIPTION_URL,
};
use toogl_core::subscription::{
    ensure_wired, fetch_subscription, parse_nodes_text, validate_subscription_url,
    write_nodes_file, SubscriptionError,
};

use crate::commands::proxy::mihomo_err;
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

/// SubscriptionError → CommandError：Fetch 归 network（401/403 归 unauthorized），
/// Parse/Link/Io/Wiring 归 internal（Wiring 的文案本身已引导用户手工接线）
fn sub_err(e: SubscriptionError) -> CommandError {
    match e {
        SubscriptionError::Fetch(m) if m.contains("HTTP 401") || m.contains("HTTP 403") => {
            CommandError::unauthorized(m)
        }
        SubscriptionError::Fetch(m) => CommandError::network(m),
        other => CommandError::internal(other.to_string()),
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
    let normalized = validate_subscription_url(&url).map_err(sub_err)?;
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

// ---- 编排主体（自由函数，命令路径与定时任务路径共用） ----

/// 订阅更新全流程：拉取 → 解析 → 与手动节点并集 → 写托管文件 → 接线 → 热重载 →
/// 落盘订阅元信息。并发进入直接拒绝（internal「正在更新中」），不排队。
///
/// 失败语义：write_nodes_file 之前失败，旧托管节点文件天然不动；之后失败
/// （接线/热重载）文件已是新节点但内核可能仍跑旧配置——记 last_error 提示，
/// 下次成功更新或 sidecar 重启（启动即读文件）后自愈。任何失败都写
/// KEY_SUBSCRIPTION_LAST_ERROR 并向上抛；成功则清 last_error。
pub async fn run_update(
    store: &ProxySettingStore,
    work_dir: &Path,
    config_path: &Path,
    client: &MihomoClient,
    updating: &AtomicBool,
) -> Result<SubscriptionInfo, CommandError> {
    if updating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(CommandError::internal("订阅正在更新中，请稍后再试"));
    }
    let result = run_update_locked(store, work_dir, config_path, client).await;
    updating.store(false, Ordering::SeqCst);
    result
}

async fn run_update_locked(
    store: &ProxySettingStore,
    work_dir: &Path,
    config_path: &Path,
    client: &MihomoClient,
) -> Result<SubscriptionInfo, CommandError> {
    let url = store
        .get(KEY_SUBSCRIPTION_URL)
        .await?
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| CommandError::not_found("未配置订阅链接"))?;

    let outcome: Result<(), CommandError> = async {
        let content = fetch_subscription(&url).await.map_err(sub_err)?;
        let parsed = parse_nodes_text(&content).map_err(sub_err)?;
        let manual = read_nodes_kv(store, KEY_MANUAL_NODES).await?;
        let merged = merge_nodes(&manual, &parsed.nodes);
        write_nodes_file(work_dir, &merged).map_err(sub_err)?;
        // 接线仅对本工具模板配置自动进行；用户手写配置 Wiring 错误透传，
        // 文案（core 侧）已引导手工补 proxy-providers 接线
        ensure_wired(config_path).map_err(sub_err)?;
        client.reload_config(true).await.map_err(mihomo_err)?;
        // 全部生效后才落盘订阅节点列表与元信息（保证 KV 与内核实际节点一致）
        let nodes_json = nodes_to_json(&parsed.nodes).map_err(CommandError::internal)?;
        store.set(KEY_SUBSCRIPTION_NODES, &nodes_json).await?;
        store
            .set(KEY_SUBSCRIPTION_LAST_UPDATED, &now_rfc3339())
            .await?;
        store.delete(KEY_SUBSCRIPTION_LAST_ERROR).await?;
        Ok(())
    }
    .await;

    if let Err(e) = outcome {
        // last_error 记录失败文案供前端降级展示；写库失败只记日志，原错误优先抛
        if let Err(db_err) = store.set(KEY_SUBSCRIPTION_LAST_ERROR, &e.message).await {
            eprintln!("[subscription] 写入 last_error 失败: {db_err}");
        }
        return Err(e);
    }
    // 出口处 updating 标志随即由调用方复位，返回的状态直接按 false 报
    read_status(store, false).await
}

/// 粘贴导入全流程：解析（纯 CPU，不占锁）→ 合并进手动节点（同名覆盖）→
/// 与订阅节点并集重写托管文件 → 接线 → 热重载 → 落盘手动节点列表。
pub async fn import_merge(
    store: &ProxySettingStore,
    work_dir: &Path,
    config_path: &Path,
    client: &MihomoClient,
    updating: &AtomicBool,
    text: &str,
) -> Result<ImportResult, CommandError> {
    let parsed = parse_nodes_text(text).map_err(sub_err)?;
    if updating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(CommandError::internal("订阅正在更新中，请稍后再试"));
    }
    let result = import_merge_locked(store, work_dir, config_path, client, parsed).await;
    updating.store(false, Ordering::SeqCst);
    result
}

async fn import_merge_locked(
    store: &ProxySettingStore,
    work_dir: &Path,
    config_path: &Path,
    client: &MihomoClient,
    parsed: toogl_core::subscription::ParsedNodes,
) -> Result<ImportResult, CommandError> {
    let mut manual = read_nodes_kv(store, KEY_MANUAL_NODES).await?;
    // 新粘贴的覆盖同名旧手动节点（用户重新粘贴通常是为了更新该节点配置）
    let new_names: HashSet<String> = parsed.nodes.iter().filter_map(node_name).collect();
    manual.retain(|m| !node_name(m).is_some_and(|n| new_names.contains(&n)));
    let imported = parsed.nodes.len() as u32;
    manual.extend(parsed.nodes.iter().cloned());

    let subscription = read_nodes_kv(store, KEY_SUBSCRIPTION_NODES).await?;
    let merged = merge_nodes(&manual, &subscription);
    write_nodes_file(work_dir, &merged).map_err(sub_err)?;
    ensure_wired(config_path).map_err(sub_err)?;
    client.reload_config(true).await.map_err(mihomo_err)?;

    let json = nodes_to_json(&manual).map_err(CommandError::internal)?;
    store.set(KEY_MANUAL_NODES, &json).await?;
    Ok(ImportResult {
        imported,
        failures: parsed.failures,
    })
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
    match last
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
    {
        None => true,
        Some(t) => {
            chrono::Utc::now().signed_duration_since(t.with_timezone(&chrono::Utc))
                > chrono::Duration::hours(24)
        }
    }
}

async fn set_last_error(store: &ProxySettingStore, msg: &str) {
    if let Err(e) = store.set(KEY_SUBSCRIPTION_LAST_ERROR, msg).await {
        eprintln!("[subscription] 写入 last_error 失败: {e}");
    }
}

// ---- 小工具 ----

/// 节点列表读取（proxy_setting JSON → Vec<Mapping>）。存 JSON 串（KEY_SELECTION 先例）；
/// 经 serde_json::Value 中转做 YAML↔JSON 互转，数字类型（port 等）可无损往返。
async fn read_nodes_kv(store: &ProxySettingStore, key: &str) -> Result<Vec<Mapping>, CommandError> {
    let Some(json) = store.get(key).await? else {
        return Ok(vec![]);
    };
    let raw: Vec<serde_json::Value> = serde_json::from_str(&json)
        .map_err(|e| CommandError::internal(format!("节点列表 JSON 解析失败（{key}）: {e}")))?;
    raw.into_iter()
        .map(|v| match serde_yaml::to_value(&v) {
            Ok(Value::Mapping(m)) => Ok(m),
            _ => Err(CommandError::internal(format!("节点条目结构非法（{key}）"))),
        })
        .collect()
}

/// 节点列表 → JSON 串（Vec<serde_json::Value> 中转，见 read_nodes_kv）
fn nodes_to_json(nodes: &[Mapping]) -> Result<String, String> {
    let values: Vec<serde_json::Value> = nodes
        .iter()
        .map(|m| serde_json::to_value(Value::Mapping(m.clone())))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("节点列表序列化失败: {e}"))?;
    serde_json::to_string(&values).map_err(|e| format!("节点列表序列化失败: {e}"))
}

/// 节点名（Mapping 的 name 字段；缺失返回 None，调用方自行容错）
fn node_name(m: &Mapping) -> Option<String> {
    m.get(Value::String("name".to_string()))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// 手动节点 ∪ 订阅节点：按 name 去重，**手动优先**（手动条目通常是用户精修过的
/// 个别节点，订阅同名让位）。顺序：手动在前、订阅在后。
fn merge_nodes(manual: &[Mapping], subscription: &[Mapping]) -> Vec<Mapping> {
    let manual_names: HashSet<String> = manual.iter().filter_map(node_name).collect();
    let mut out: Vec<Mapping> = manual.to_vec();
    out.extend(
        subscription
            .iter()
            .filter(|m| !node_name(m).is_some_and(|n| manual_names.contains(&n)))
            .cloned(),
    );
    out
}

/// 状态读取（节点数 JSON 损坏时降级为 0 并记日志——状态查询不因脏数据整体报错）
async fn read_status(
    store: &ProxySettingStore,
    updating: bool,
) -> Result<SubscriptionInfo, CommandError> {
    Ok(SubscriptionInfo {
        url: store.get(KEY_SUBSCRIPTION_URL).await?,
        last_updated: store.get(KEY_SUBSCRIPTION_LAST_UPDATED).await?,
        node_count: count_nodes(store, KEY_SUBSCRIPTION_NODES).await,
        manual_count: count_nodes(store, KEY_MANUAL_NODES).await,
        updating,
        last_error: store.get(KEY_SUBSCRIPTION_LAST_ERROR).await?,
    })
}

async fn count_nodes(store: &ProxySettingStore, key: &str) -> u32 {
    match store.get(key).await {
        Ok(Some(json)) => match serde_json::from_str::<Vec<serde_json::Value>>(&json) {
            Ok(v) => v.len() as u32,
            Err(e) => {
                eprintln!("[subscription] 节点列表 JSON 损坏（{key}），按 0 计: {e}");
                0
            }
        },
        _ => 0,
    }
}

/// 上次成功更新时间格式：RFC3339 UTC 秒级（与 quota_snapshot 的 fetched_at 一致）
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const LINK_HK: &str = "vless://uuid-1@hk.example.com:443?security=tls#%E9%A6%99%E6%B8%AF%2001";
    const LINK_JP: &str = "vless://uuid-2@jp.example.com:8443?security=tls#%E6%97%A5%E6%9C%AC%2001";

    /// 测试夹具：临时目录 + 真实 ProxySettingStore（tempfile db）+ 带模板标记的
    /// config.yaml（ensure_wired 自动接线的前提）+ 更新标志
    async fn fixture() -> (
        tempfile::TempDir,
        ProxySettingStore,
        PathBuf,
        PathBuf,
        AtomicBool,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProxySettingStore::connect(tmp.path().join("toogl.db"))
            .await
            .unwrap();
        let work_dir = tmp.path().join("mihomo");
        std::fs::create_dir_all(&work_dir).unwrap();
        let config_path = work_dir.join("config.yaml");
        std::fs::write(
            &config_path,
            "# 本文件由 Toogl 首次运行自动生成，可手工修改；已存在时不会被覆盖。\n\
             mixed-port: 7890\n\
             proxies: []\n\
             proxy-groups:\n  - name: 节点选择\n    type: select\n    proxies:\n      - DIRECT\n\
             rules:\n  - MATCH,节点选择\n",
        )
        .unwrap();
        (tmp, store, work_dir, config_path, AtomicBool::new(false))
    }

    /// 起一对 mock：订阅服务器（返回指定内容/状态码）+ mihomo controller（reload 204）
    async fn mock_servers(
        sub_status: usize,
        sub_body: &str,
    ) -> (MockServer, MockServer, MihomoClient) {
        let sub = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(sub_status as u16).set_body_string(sub_body))
            .mount(&sub)
            .await;
        let mihomo = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/configs"))
            .and(query_param("force", "true"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&mihomo)
            .await;
        let client = MihomoClient::new(mihomo.uri(), None);
        (sub, mihomo, client)
    }

    fn kv_json_names(json: &str) -> Vec<String> {
        serde_json::from_str::<Vec<serde_json::Value>>(json)
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap().to_string())
            .collect()
    }

    // ---- run_update ----

    #[tokio::test]
    async fn run_update_full_success_flow() {
        let (_tmp, store, work_dir, config_path, updating) = fixture().await;
        let (sub, mihomo, client) = mock_servers(200, &format!("{LINK_HK}\n{LINK_JP}\n")).await;
        store
            .set(KEY_SUBSCRIPTION_URL, &format!("{}/feed", sub.uri()))
            .await
            .unwrap();

        let info = run_update(&store, &work_dir, &config_path, &client, &updating)
            .await
            .unwrap();
        assert_eq!(info.node_count, 2);
        assert_eq!(info.manual_count, 0);
        assert_eq!(info.last_error, None);
        assert!(info.last_updated.is_some());
        assert!(!info.updating);

        // KV：订阅节点 JSON 两条、last_updated 为 RFC3339、last_error 已清
        let nodes_json = store.get(KEY_SUBSCRIPTION_NODES).await.unwrap().unwrap();
        assert_eq!(kv_json_names(&nodes_json), ["香港 01", "日本 01"]);
        let ts = store
            .get(KEY_SUBSCRIPTION_LAST_UPDATED)
            .await
            .unwrap()
            .unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&ts).is_ok());
        assert_eq!(store.get(KEY_SUBSCRIPTION_LAST_ERROR).await.unwrap(), None);

        // 托管节点文件：注释头 + 两个节点；config.yaml 已接线
        let file = std::fs::read_to_string(work_dir.join("toogl-nodes.yaml")).unwrap();
        assert!(file.starts_with("# 本文件由 Toogl 托管"));
        assert!(file.contains("name: 香港 01") && file.contains("name: 日本 01"));
        let config = std::fs::read_to_string(&config_path).unwrap();
        assert!(config.contains("proxy-providers:") && config.contains("toogl"));

        // reload 被恰好调用一次（mock expect(1)）
        mihomo.verify().await;
        // 互斥标志已复位
        assert!(!updating.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn run_update_fetch_failure_preserves_old_file_and_records_error() {
        let (_tmp, store, work_dir, config_path, updating) = fixture().await;
        // 先成功一次（订阅含香港 01）
        let (sub, mihomo, client) = mock_servers(200, LINK_HK).await;
        store
            .set(KEY_SUBSCRIPTION_URL, &format!("{}/feed", sub.uri()))
            .await
            .unwrap();
        run_update(&store, &work_dir, &config_path, &client, &updating)
            .await
            .unwrap();
        mihomo.verify().await;

        // 订阅服务器改挂 500（wiremock 先挂载的 mock 优先匹配，需先 reset 清掉 200 mock）
        sub.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&sub)
            .await;
        let err = run_update(&store, &work_dir, &config_path, &client, &updating)
            .await
            .unwrap_err();
        assert_eq!(err.kind, "network");
        assert!(err.message.contains("500"));

        let file = std::fs::read_to_string(work_dir.join("toogl-nodes.yaml")).unwrap();
        assert!(file.contains("name: 香港 01"));
        let nodes_json = store.get(KEY_SUBSCRIPTION_NODES).await.unwrap().unwrap();
        assert_eq!(kv_json_names(&nodes_json), ["香港 01"]);
        let last_error = store
            .get(KEY_SUBSCRIPTION_LAST_ERROR)
            .await
            .unwrap()
            .unwrap();
        assert!(last_error.contains("500"));
    }

    #[tokio::test]
    async fn run_update_without_url_is_not_found() {
        let (_tmp, store, work_dir, config_path, updating) = fixture().await;
        let mihomo = MockServer::start().await;
        let client = MihomoClient::new(mihomo.uri(), None);
        let err = run_update(&store, &work_dir, &config_path, &client, &updating)
            .await
            .unwrap_err();
        assert_eq!(err.kind, "not_found");
        assert!(err.message.contains("未配置订阅链接"));
    }

    #[tokio::test]
    async fn run_update_rejects_concurrent_entry() {
        let (_tmp, store, work_dir, config_path, updating) = fixture().await;
        updating.store(true, Ordering::SeqCst); // 模拟另一路径正在更新
        let mihomo = MockServer::start().await;
        let client = MihomoClient::new(mihomo.uri(), None);
        let err = run_update(&store, &work_dir, &config_path, &client, &updating)
            .await
            .unwrap_err();
        assert_eq!(err.kind, "internal");
        assert!(err.message.contains("正在更新中"));
    }

    // ---- import_merge ----

    #[tokio::test]
    async fn import_merge_dedupes_and_manual_overrides_subscription() {
        let (_tmp, store, work_dir, config_path, updating) = fixture().await;
        // 预置：手动节点「香港 01」（旧 server）、订阅节点「香港 01」（订阅版）+「美国 01」
        let old_hk =
            parse_nodes_text("vless://uuid-old@old-hk.example.com:443#%E9%A6%99%E6%B8%AF%2001")
                .unwrap();
        store
            .set(KEY_MANUAL_NODES, &nodes_to_json(&old_hk.nodes).unwrap())
            .await
            .unwrap();
        let sub_nodes = parse_nodes_text(
            "vless://uuid-sub@sub-hk.example.com:443#%E9%A6%99%E6%B8%AF%2001\n\
             vless://uuid-us@us.example.com:443#%E7%BE%8E%E5%9B%BD%2001\n",
        )
        .unwrap();
        store
            .set(
                KEY_SUBSCRIPTION_NODES,
                &nodes_to_json(&sub_nodes.nodes).unwrap(),
            )
            .await
            .unwrap();

        let mihomo = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/configs"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&mihomo)
            .await;
        let client = MihomoClient::new(mihomo.uri(), None);

        // 粘贴：同名「香港 01」（新 server）+「日本 01」+ 一条坏行
        let text = format!("{LINK_HK}\n{LINK_JP}\nvmess://bad\n");
        let result = import_merge(&store, &work_dir, &config_path, &client, &updating, &text)
            .await
            .unwrap();
        assert_eq!(result.imported, 2);
        assert_eq!(result.failures.len(), 1);
        assert!(result.failures[0].reason.contains("vmess"));

        // 手动 KV：同名覆盖（新 server）+ 新增，共 2 条
        let manual_json = store.get(KEY_MANUAL_NODES).await.unwrap().unwrap();
        assert_eq!(kv_json_names(&manual_json), ["香港 01", "日本 01"]);
        assert!(manual_json.contains("hk.example.com"));
        assert!(!manual_json.contains("old-hk.example.com"));

        // 数字类型往返：port 读回仍是 YAML 整数
        let manual = read_nodes_kv(&store, KEY_MANUAL_NODES).await.unwrap();
        assert_eq!(manual[1]["port"], Value::Number(8443.into()));

        // 托管文件：手动「香港 01」覆盖订阅同名（只出现一次且是手动版），
        // 订阅独有的「美国 01」并入
        let file = std::fs::read_to_string(work_dir.join("toogl-nodes.yaml")).unwrap();
        assert_eq!(file.matches("name: 香港 01").count(), 1);
        assert!(file.contains("hk.example.com"));
        assert!(!file.contains("sub-hk.example.com"));
        assert!(file.contains("name: 美国 01") && file.contains("name: 日本 01"));
        mihomo.verify().await;
    }

    // ---- 错误映射与辅助函数 ----

    #[test]
    fn sub_err_mapping() {
        assert_eq!(
            sub_err(SubscriptionError::Fetch("HTTP 401".into())).kind,
            "unauthorized"
        );
        assert_eq!(
            sub_err(SubscriptionError::Fetch("HTTP 403".into())).kind,
            "unauthorized"
        );
        assert_eq!(
            sub_err(SubscriptionError::Fetch("超时".into())).kind,
            "network"
        );
        assert_eq!(
            sub_err(SubscriptionError::Parse("坏内容".into())).kind,
            "internal"
        );
        assert_eq!(
            sub_err(SubscriptionError::Wiring("手工接线".into())).kind,
            "internal"
        );
    }

    #[test]
    fn merge_nodes_manual_wins_on_same_name() {
        let manual = parse_nodes_text("vless://u@m.example.com:443#%E5%90%8C%E5%90%8D").unwrap();
        let sub = parse_nodes_text(
            "vless://u@s.example.com:443#%E5%90%8C%E5%90%8D\nvless://u@o.example.com:443#%E7%8B%AC%E6%9C%89\n",
        )
        .unwrap();
        let merged = merge_nodes(&manual.nodes, &sub.nodes);
        let names: Vec<String> = merged.iter().filter_map(node_name).collect();
        assert_eq!(names, ["同名", "独有"]);
        assert_eq!(
            merged[0]["server"],
            Value::String("m.example.com".to_string())
        );
    }

    #[tokio::test]
    async fn should_refresh_logic() {
        let (_tmp, store, _w, _c, _u) = fixture().await;
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
        // 超期（25h 前）→ 刷
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
        let (_tmp, store, _w, _c, _u) = fixture().await;
        // 无端口记录 → None
        assert!(controller_client(&store).await.is_none());
        store.set(KEY_CONTROLLER_PORT, "9095").await.unwrap();
        store.set(KEY_CONTROLLER_SECRET, "s3cret").await.unwrap();
        let client = controller_client(&store).await.unwrap();
        assert_eq!(client.base_url(), "http://127.0.0.1:9095");
    }
}
