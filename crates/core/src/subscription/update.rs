//! 订阅更新 / 粘贴导入编排（M5-07）：命令路径与 24h 定时任务路径共用同一实现。
//!
//! 全流程：拉取 → 解析 → 与手动节点并集（手动优先去重）→ ensure_wired 接线 →
//!（备份旧文件）原子写 toogl-nodes.yaml → mihomo 热重载（失败回滚旧文件）→
//! 落盘订阅元信息（proxy_setting KV）。并发进入由调用方传入的 AtomicBool 抢占拒绝。
//!
//! 一致性窗口：接线提前到写文件之前（接线失败时托管文件不动），写文件与热重载之间
//! 是唯一的「文件与内核不一致」窗口，热重载失败时把备份的旧文件内容原子写回，
//! 保证磁盘文件与内核跑的始终是同一份。

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_yaml::{Mapping, Value};

use super::config::{ensure_wired, restore_nodes_file, write_nodes_file, NODES_FILE_NAME};
use super::content::{parse_nodes_text, ParsedNodes};
use super::fetch::fetch_subscription;
use crate::mihomo::MihomoClient;
use crate::models::{CommandError, ImportResult, SubscriptionInfo};
use crate::proxy_setting::{
    ProxySettingStore, KEY_MANUAL_NODES, KEY_SUBSCRIPTION_LAST_ERROR,
    KEY_SUBSCRIPTION_LAST_UPDATED, KEY_SUBSCRIPTION_NODES, KEY_SUBSCRIPTION_URL,
};

/// 订阅更新全流程：拉取 → 解析 → 与手动节点并集 → 接线 →（备份旧文件）写托管文件 →
/// 热重载（失败回滚旧文件）→ 落盘订阅元信息。并发进入直接拒绝（internal「正在更新中」），不排队。
///
/// 失败语义：write_nodes_file 之前失败，旧托管节点文件天然不动（接线已提前到写文件之前，
/// 接线失败时文件也不动）；热重载失败时把备份的旧文件内容原子写回（restore 失败则在错误
/// 文案里追加说明），保证磁盘文件与内核跑的始终是同一份。任何失败都写
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
        let content = fetch_subscription(&url).await?;
        let parsed = parse_nodes_text(&content)?;
        let manual = read_nodes_kv(store, KEY_MANUAL_NODES).await?;
        let merged = merge_nodes(&manual, &parsed.nodes);
        // 接线提前到写文件之前：接线失败时托管节点文件不动，
        // 「文件与内核不一致」的窗口只剩热重载一步（失败走下面回滚）。
        // 接线仅对本工具模板配置自动进行；用户手写配置 Wiring 错误透传，
        // 文案（config 模块）已引导手工补 proxy-providers 接线
        ensure_wired(config_path)?;
        // 备份旧托管文件原始内容（不存在记 None），热重载失败时原子写回
        let old_nodes = std::fs::read_to_string(work_dir.join(NODES_FILE_NAME)).ok();
        write_nodes_file(work_dir, &merged)?;
        if let Err(e) = client.reload_config(true).await {
            let mut err = CommandError::from(e);
            match restore_nodes_file(work_dir, old_nodes.as_deref()) {
                Ok(()) => err.message.push_str("；已回滚节点文件到更新前版本"),
                Err(re) => err.message.push_str(&format!(
                    "；节点文件回滚失败（文件与内核配置不一致，重启后自愈）: {re}"
                )),
            }
            return Err(err);
        }
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
/// 与订阅节点并集 → 接线 →（备份旧文件）重写托管文件 → 热重载（失败回滚旧文件）→
/// 落盘手动节点列表。一致性窗口处理与 run_update 相同（接线提前 + reload 失败回滚）。
pub async fn import_merge(
    store: &ProxySettingStore,
    work_dir: &Path,
    config_path: &Path,
    client: &MihomoClient,
    updating: &AtomicBool,
    text: &str,
) -> Result<ImportResult, CommandError> {
    let parsed = parse_nodes_text(text)?;
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
    parsed: ParsedNodes,
) -> Result<ImportResult, CommandError> {
    let mut manual = read_nodes_kv(store, KEY_MANUAL_NODES).await?;
    // 新粘贴的覆盖同名旧手动节点（用户重新粘贴通常是为了更新该节点配置）
    let new_names: HashSet<String> = parsed.nodes.iter().filter_map(node_name).collect();
    manual.retain(|m| !node_name(m).is_some_and(|n| new_names.contains(&n)));
    let imported = parsed.nodes.len() as u32;
    manual.extend(parsed.nodes.iter().cloned());

    let subscription = read_nodes_kv(store, KEY_SUBSCRIPTION_NODES).await?;
    let merged = merge_nodes(&manual, &subscription);
    // 接线提前 + 热重载失败回滚，与 run_update 同一份一致性保证
    ensure_wired(config_path)?;
    let old_nodes = std::fs::read_to_string(work_dir.join(NODES_FILE_NAME)).ok();
    write_nodes_file(work_dir, &merged)?;
    if let Err(e) = client.reload_config(true).await {
        let mut err = CommandError::from(e);
        match restore_nodes_file(work_dir, old_nodes.as_deref()) {
            Ok(()) => err.message.push_str("；已回滚节点文件到导入前版本"),
            Err(re) => err.message.push_str(&format!(
                "；节点文件回滚失败（文件与内核配置不一致，重启后自愈）: {re}"
            )),
        }
        return Err(err);
    }

    let json = nodes_to_json(&manual).map_err(CommandError::internal)?;
    store.set(KEY_MANUAL_NODES, &json).await?;
    Ok(ImportResult {
        imported,
        failures: parsed.failures,
    })
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
pub async fn read_status(
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

/// 上次成功更新时间格式：RFC3339 UTC 秒级。
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CommandErrorKind;
    use crate::subscription::parse_nodes_text;
    use std::path::PathBuf;
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
        assert_eq!(err.kind, CommandErrorKind::Network);
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

    /// 订阅拉取 401/403 → unauthorized（按 FetchHttp 状态码归类，不再靠字符串嗅探）
    #[tokio::test]
    async fn run_update_fetch_401_maps_to_unauthorized() {
        let (_tmp, store, work_dir, config_path, updating) = fixture().await;
        // 订阅服务器挂 401；fetch 失败走不到热重载，mihomo 侧只提供 client 目标 URL
        let sub = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&sub)
            .await;
        let mihomo = MockServer::start().await;
        let client = MihomoClient::new(mihomo.uri(), None);
        store
            .set(KEY_SUBSCRIPTION_URL, &format!("{}/feed", sub.uri()))
            .await
            .unwrap();
        let err = run_update(&store, &work_dir, &config_path, &client, &updating)
            .await
            .unwrap_err();
        assert_eq!(err.kind, CommandErrorKind::Unauthorized);
        assert!(err.message.contains("401"));
    }

    /// 热重载失败：托管文件回滚到更新前的旧内容（文件与内核保持同一份），
    /// last_error 落盘（含回滚说明），错误向上抛
    #[tokio::test]
    async fn run_update_reload_failure_rolls_back_file_and_records_error() {
        let (_tmp, store, work_dir, config_path, updating) = fixture().await;
        // 先成功一次（订阅含香港 01，托管文件落盘旧版本）
        let (sub, mihomo, client) = mock_servers(200, LINK_HK).await;
        store
            .set(KEY_SUBSCRIPTION_URL, &format!("{}/feed", sub.uri()))
            .await
            .unwrap();
        run_update(&store, &work_dir, &config_path, &client, &updating)
            .await
            .unwrap();
        mihomo.verify().await;
        let old_file = std::fs::read_to_string(work_dir.join(NODES_FILE_NAME)).unwrap();
        assert!(old_file.contains("name: 香港 01"));

        // 订阅内容换成日本 01；mihomo 热重载改挂 500
        // （wiremock 先挂载的 mock 优先匹配，两侧都需先 reset 再挂新 mock）
        sub.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(LINK_JP))
            .mount(&sub)
            .await;
        mihomo.reset().await;
        Mock::given(method("PUT"))
            .and(path("/configs"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mihomo)
            .await;

        let err = run_update(&store, &work_dir, &config_path, &client, &updating)
            .await
            .unwrap_err();
        assert_eq!(err.kind, CommandErrorKind::Internal);
        assert!(err.message.contains("已回滚"));

        // 文件内容恢复旧值（与首次写入逐字节一致）
        let file = std::fs::read_to_string(work_dir.join(NODES_FILE_NAME)).unwrap();
        assert_eq!(file, old_file);
        // last_error 落盘且带回滚说明
        let last_error = store
            .get(KEY_SUBSCRIPTION_LAST_ERROR)
            .await
            .unwrap()
            .unwrap();
        assert!(last_error.contains("已回滚"));
        // 订阅节点 KV 未更新（仍是香港 01），与文件/内核一致
        let nodes_json = store.get(KEY_SUBSCRIPTION_NODES).await.unwrap().unwrap();
        assert_eq!(kv_json_names(&nodes_json), ["香港 01"]);
    }

    /// 热重载失败且更新前无托管文件（首次更新）：回滚即删除新文件，不残留
    #[tokio::test]
    async fn run_update_reload_failure_without_old_file_removes_new_file() {
        let (_tmp, store, work_dir, config_path, updating) = fixture().await;
        let sub = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(LINK_HK))
            .mount(&sub)
            .await;
        let mihomo = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/configs"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mihomo)
            .await;
        let client = MihomoClient::new(mihomo.uri(), None);
        store
            .set(KEY_SUBSCRIPTION_URL, &format!("{}/feed", sub.uri()))
            .await
            .unwrap();

        let err = run_update(&store, &work_dir, &config_path, &client, &updating)
            .await
            .unwrap_err();
        assert_eq!(err.kind, CommandErrorKind::Internal);
        // 新写入的托管文件已被回滚删除
        assert!(!work_dir.join(NODES_FILE_NAME).exists());
        assert!(store
            .get(KEY_SUBSCRIPTION_LAST_ERROR)
            .await
            .unwrap()
            .unwrap()
            .contains("已回滚"));
    }

    #[tokio::test]
    async fn run_update_without_url_is_not_found() {
        let (_tmp, store, work_dir, config_path, updating) = fixture().await;
        let mihomo = MockServer::start().await;
        let client = MihomoClient::new(mihomo.uri(), None);
        let err = run_update(&store, &work_dir, &config_path, &client, &updating)
            .await
            .unwrap_err();
        assert_eq!(err.kind, CommandErrorKind::NotFound);
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
        assert_eq!(err.kind, CommandErrorKind::Internal);
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

    /// 粘贴导入热重载失败：托管文件回滚到导入前内容，手动节点 KV 不写入
    #[tokio::test]
    async fn import_merge_reload_failure_rolls_back_file() {
        let (_tmp, store, work_dir, config_path, updating) = fixture().await;
        let mihomo = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/configs"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&mihomo)
            .await;
        let client = MihomoClient::new(mihomo.uri(), None);

        // 首次导入成功（香港 01）
        import_merge(&store, &work_dir, &config_path, &client, &updating, LINK_HK)
            .await
            .unwrap();
        mihomo.verify().await;
        let old_file = std::fs::read_to_string(work_dir.join(NODES_FILE_NAME)).unwrap();

        // 第二次导入（日本 01）热重载 500（先 reset 再挂，先挂的 mock 优先匹配）
        mihomo.reset().await;
        Mock::given(method("PUT"))
            .and(path("/configs"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mihomo)
            .await;
        let err = import_merge(&store, &work_dir, &config_path, &client, &updating, LINK_JP)
            .await
            .unwrap_err();
        assert!(err.message.contains("已回滚"));
        // 文件回滚为导入前内容；手动节点 KV 未写入（仍是香港 01）
        assert_eq!(
            std::fs::read_to_string(work_dir.join(NODES_FILE_NAME)).unwrap(),
            old_file
        );
        let manual_json = store.get(KEY_MANUAL_NODES).await.unwrap().unwrap();
        assert_eq!(kv_json_names(&manual_json), ["香港 01"]);
    }

    // ---- 错误映射与辅助函数 ----

    #[test]
    fn subscription_error_maps_to_command_error() {
        assert_eq!(
            CommandError::from(crate::subscription::SubscriptionError::FetchHttp(401)).kind,
            CommandErrorKind::Unauthorized
        );
        assert_eq!(
            CommandError::from(crate::subscription::SubscriptionError::FetchHttp(403)).kind,
            CommandErrorKind::Unauthorized
        );
        assert_eq!(
            CommandError::from(crate::subscription::SubscriptionError::FetchHttp(500)).kind,
            CommandErrorKind::Network
        );
        assert_eq!(
            CommandError::from(crate::subscription::SubscriptionError::Fetch("超时".into())).kind,
            CommandErrorKind::Network
        );
        assert_eq!(
            CommandError::from(crate::subscription::SubscriptionError::Parse(
                "坏内容".into()
            ))
            .kind,
            CommandErrorKind::Internal
        );
        assert_eq!(
            CommandError::from(crate::subscription::SubscriptionError::Wiring(
                "手工接线".into()
            ))
            .kind,
            CommandErrorKind::Internal
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
}
