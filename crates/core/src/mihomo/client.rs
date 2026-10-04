//! mihomo RESTful API 客户端实现（M5 核心三件套的远端一半；系统代理在 sysproxy 模块）。
//! 端点覆盖：GET /proxies、PUT /proxies/{group}、GET /proxies/{name}/delay、
//! PATCH /configs、PUT /configs?force=（热重载）、GET /version。reqwest client 进程级复用（同 adapters 的静态模式）。
//! 鉴权：构造时传入 external-controller secret（Option），所有请求自动带
//! `Authorization: Bearer <secret>` 头；未配置 secret 的旧部署不带头。

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

use super::MihomoError;
use crate::models::{
    DelayStatus, MihomoVersion, NodeDelayResult, ProxiesSummary, ProxyGroup, ProxyGroupType,
    ProxyNode, RuleMode,
};

/// 默认测速目标（mihomo 官方默认的 204 探测地址）
pub const DEFAULT_TEST_URL: &str = "https://www.gstatic.com/generate_204";
/// 单节点测速默认超时（毫秒），作为 query 参数传给 mihomo 由其内核执行
pub const DEFAULT_DELAY_TIMEOUT_MS: u32 = 5000;
/// HTTP 客户端整体超时（本地回环服务，10s 已非常宽松）
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// 进程级共享的 reqwest 客户端（连接池复用，避免每次新建）
static SHARED_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn shared_client() -> reqwest::Client {
    SHARED_CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(DEFAULT_TIMEOUT)
                .build()
                .expect("reqwest client 构建失败")
        })
        .clone()
}

#[derive(Debug, Clone)]
pub struct MihomoClient {
    base_url: String,
    /// external-controller 鉴权密钥（mihomo 配置 secret 字段）；
    /// Some 时所有请求带 `Authorization: Bearer <secret>` 头
    secret: Option<String>,
    client: reqwest::Client,
}

impl MihomoClient {
    pub fn new(base_url: impl Into<String>, secret: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            secret,
            client: shared_client(),
        }
    }

    /// 自定义超时的独立客户端（主要用于测试超时路径）
    pub fn with_timeout(
        base_url: impl Into<String>,
        secret: Option<String>,
        timeout: Duration,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client 构建失败");
        Self {
            base_url: base_url.into(),
            secret,
            client,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 配置了 secret 时为请求附带 Bearer 鉴权头，未配置则原样返回
    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.secret {
            Some(secret) => req.bearer_auth(secret),
            None => req,
        }
    }

    /// 拼接端点 URL；组名/节点名经 path segment 编码（名字可含空格与 emoji）
    fn endpoint(&self, segments: &[&str]) -> Result<reqwest::Url, MihomoError> {
        let mut url = reqwest::Url::parse(self.base_url.trim_end_matches('/'))
            .map_err(|e| MihomoError::Unreachable(format!("base_url 非法: {e}")))?;
        url.path_segments_mut()
            .map_err(|_| MihomoError::Unreachable("base_url 不支持相对路径".to_string()))?
            .extend(segments.iter().copied());
        Ok(url)
    }

    /// 发送请求并统一处理错误：网络层错误 → Unreachable，非 2xx → Api（带截断的错误体）
    async fn send(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response, MihomoError> {
        let resp = req
            .send()
            .await
            .map_err(|e| MihomoError::Unreachable(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            // mihomo 错误体通常为 {"message": "..."}，截断防超长日志
            let snippet: String = body.chars().take(200).collect();
            return Err(MihomoError::Api(format!("{status}: {snippet}")));
        }
        Ok(resp)
    }

    /// GET /proxies：代理组与节点摘要。
    /// 组 = 响应中带 all 数组的条目（含 GLOBAL）；节点延迟取 history 末条
    /// （meanDelay 优先，退化为 delay；值为 0 视为超时，空 history 为无记录）。
    pub async fn get_proxies(&self) -> Result<ProxiesSummary, MihomoError> {
        let resp = self
            .send(self.authed(self.client.get(self.endpoint(&["proxies"])?)))
            .await?;
        let body: ProxiesResponse = resp
            .json()
            .await
            .map_err(|e| MihomoError::Parse(e.to_string()))?;

        let mut groups: Vec<ProxyGroup> = Vec::new();
        for (key, raw) in &body.proxies {
            let Some(all) = &raw.all else {
                continue; // 无 all 数组的是普通节点条目，跳过
            };
            let nodes = all
                .iter()
                .map(|name| match body.proxies.get(name) {
                    Some(member) => node_from_raw(member),
                    // all 引用了响应里不存在的条目（配置异常）：给占位节点，不丢弃整组
                    None => ProxyNode {
                        name: name.clone(),
                        node_type: String::new(),
                        delay_ms: None,
                        delay_status: DelayStatus::Untested,
                    },
                })
                .collect();
            groups.push(ProxyGroup {
                name: raw.name.clone().unwrap_or_else(|| key.clone()),
                group_type: ProxyGroupType::parse(&raw.proxy_type),
                now: raw.now.clone().unwrap_or_default(),
                nodes,
            });
        }
        // JSON object 无序，按名字排序保证前端渲染稳定
        groups.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(ProxiesSummary { groups })
    }

    /// PUT /proxies/{group}：Selector 组切换节点，body {"name": 节点名}。
    /// 组不存在或节点不在组内时 mihomo 返回 4xx，映射为 Api 错误。
    pub async fn select_proxy(&self, group: &str, node: &str) -> Result<(), MihomoError> {
        self.send(
            self.authed(
                self.client
                    .put(self.endpoint(&["proxies", group])?)
                    .json(&serde_json::json!({ "name": node })),
            ),
        )
        .await?;
        Ok(())
    }

    /// GET /proxies/{name}/delay?timeout=&url=：单节点测速。
    /// 408（内核侧超时）与 delay=0 视为超时结果而非错误；其余 4xx/5xx 为 Api 错误。
    pub async fn test_delay(
        &self,
        name: &str,
        timeout_ms: u32,
        url: &str,
    ) -> Result<NodeDelayResult, MihomoError> {
        let req = self.authed(
            self.client
                .get(self.endpoint(&["proxies", name, "delay"])?)
                .query(&[
                    ("timeout", timeout_ms.to_string()),
                    ("url", url.to_string()),
                ]),
        );
        let resp = req
            .send()
            .await
            .map_err(|e| MihomoError::Unreachable(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::REQUEST_TIMEOUT {
            return Ok(NodeDelayResult {
                name: name.to_string(),
                delay_ms: None,
                delay_status: DelayStatus::Timeout,
                error: None,
            });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(200).collect();
            return Err(MihomoError::Api(format!("{status}: {snippet}")));
        }
        let body: DelayResponse = resp
            .json()
            .await
            .map_err(|e| MihomoError::Parse(e.to_string()))?;
        Ok(delay_result(name, body.delay))
    }

    /// 批量测速：并发发起，单节点失败（含 mihomo 不可达）只影响自身结果，
    /// 不中断其余节点；返回顺序与输入 names 一致。
    pub async fn test_delays(
        &self,
        names: &[String],
        timeout_ms: u32,
        url: &str,
    ) -> Vec<NodeDelayResult> {
        // 先全部 spawn 出去保证并发，再按输入顺序逐个 await 收集结果
        let mut handles = Vec::with_capacity(names.len());
        for name in names {
            let this = self.clone();
            let node_name = name.clone();
            let url = url.to_string();
            let handle =
                tokio::spawn(async move { this.test_delay(&node_name, timeout_ms, &url).await });
            handles.push((name.clone(), handle));
        }
        let mut out = Vec::with_capacity(handles.len());
        for (name, handle) in handles {
            let result = match handle.await {
                Ok(Ok(result)) => result,
                // 请求级错误（不可达 / Api / Parse）：记入该节点结果，不拖垮整批
                Ok(Err(e)) => NodeDelayResult {
                    name,
                    delay_ms: None,
                    delay_status: DelayStatus::Timeout,
                    error: Some(e.to_string()),
                },
                // spawn 任务被取消/panic（正常不会发生）：同样降级为失败结果
                Err(e) => NodeDelayResult {
                    name,
                    delay_ms: None,
                    delay_status: DelayStatus::Timeout,
                    error: Some(format!("测速任务异常: {e}")),
                },
            };
            out.push(result);
        }
        out
    }

    /// PATCH /configs：规则模式切换，body {"mode": "rule|global|direct"}。
    pub async fn set_mode(&self, mode: RuleMode) -> Result<(), MihomoError> {
        self.send(
            self.authed(
                self.client
                    .patch(self.endpoint(&["configs"])?)
                    .json(&serde_json::json!({ "mode": mode })),
            ),
        )
        .await?;
        Ok(())
    }

    /// GET /configs：读取当前规则模式（mihomo 侧才是事实源）。
    /// mode 字段缺失或取值未知 → Parse 错误，由上层降级（退化为本地记住的模式）。
    pub async fn get_mode(&self) -> Result<RuleMode, MihomoError> {
        let resp = self
            .send(self.authed(self.client.get(self.endpoint(&["configs"])?)))
            .await?;
        let body: ConfigsResponse = resp
            .json()
            .await
            .map_err(|e| MihomoError::Parse(e.to_string()))?;
        RuleMode::parse(&body.mode)
            .ok_or_else(|| MihomoError::Parse(format!("未知规则模式: {}", body.mode)))
    }

    /// PUT /configs?force=true|false：热重载配置文件（节点导入后让内核重读 config.yaml
    /// 与 proxy-providers 指向的托管节点文件）。force=true 跳过配置缓存比较强制重载。
    /// mihomo API 要求 body 为 JSON 对象，空对象 `{}` 即可。
    pub async fn reload_config(&self, force: bool) -> Result<(), MihomoError> {
        self.send(
            self.authed(
                self.client
                    .put(self.endpoint(&["configs"])?)
                    .query(&[("force", force.to_string())])
                    .json(&serde_json::json!({})),
            ),
        )
        .await?;
        Ok(())
    }

    /// GET /version：存活探测 + 内核版本
    pub async fn version(&self) -> Result<MihomoVersion, MihomoError> {
        let resp = self
            .send(self.authed(self.client.get(self.endpoint(&["version"])?)))
            .await?;
        let body: VersionResponse = resp
            .json()
            .await
            .map_err(|e| MihomoError::Parse(e.to_string()))?;
        Ok(MihomoVersion {
            version: body.version,
            meta: body.meta,
        })
    }

    /// 存活探测（sidecar 拉起后等待就绪用）：version() 成功即存活
    pub async fn alive(&self) -> bool {
        self.version().await.is_ok()
    }
}

/// history 末条 → 节点延迟状态。mihomo 惯例：超时记录的 delay 为 0。
fn node_from_raw(raw: &RawProxy) -> ProxyNode {
    let (delay_ms, delay_status) = match raw.history.last() {
        None => (None, DelayStatus::Untested),
        Some(h) => match h.mean_delay.or(h.delay) {
            None => (None, DelayStatus::Untested),
            Some(0) => (None, DelayStatus::Timeout),
            Some(d) => (Some(d), DelayStatus::Delayed),
        },
    };
    ProxyNode {
        name: raw.name.clone().unwrap_or_default(),
        node_type: raw.proxy_type.clone(),
        delay_ms,
        delay_status,
    }
}

fn delay_result(name: &str, delay: u32) -> NodeDelayResult {
    if delay == 0 {
        NodeDelayResult {
            name: name.to_string(),
            delay_ms: None,
            delay_status: DelayStatus::Timeout,
            error: None,
        }
    } else {
        NodeDelayResult {
            name: name.to_string(),
            delay_ms: Some(delay),
            delay_status: DelayStatus::Delayed,
            error: None,
        }
    }
}

// ---- /proxies 响应的内部解析结构（字段宽松：单字段缺失降级，不拖垮整体） ----

#[derive(Debug, Deserialize)]
struct ProxiesResponse {
    proxies: HashMap<String, RawProxy>,
}

#[derive(Debug, Deserialize)]
struct RawProxy {
    /// 响应条目通常自带 name；缺失时退化为 map 的 key
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "type", default)]
    proxy_type: String,
    /// 仅代理组有此字段；节点条目没有
    #[serde(default)]
    all: Option<Vec<String>>,
    #[serde(default)]
    now: Option<String>,
    #[serde(default)]
    history: Vec<HistoryEntry>,
}

#[derive(Debug, Deserialize)]
struct HistoryEntry {
    /// 部分内核版本/分支用 meanDelay
    #[serde(default, rename = "meanDelay")]
    mean_delay: Option<u32>,
    #[serde(default)]
    delay: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct DelayResponse {
    delay: u32,
}

/// GET /configs 响应（其余字段如 port/allow-lan 不关心，serde 宽松忽略）
#[derive(Debug, Deserialize)]
struct ConfigsResponse {
    mode: String,
}

/// 收集批量测速的目标节点名（纯函数，供命令层 test_delay 使用）：
/// - group 为 None：所有 Selector / URLTest 组的节点（排除 GLOBAL 组避免全量重复测，去重保序）；
/// - Some(group)：指定组的全部节点，组不存在返回 Err（上层映射为 not_found）。
pub fn collect_test_node_names(
    summary: &ProxiesSummary,
    group: Option<&str>,
) -> Result<Vec<String>, String> {
    match group {
        Some(name) => summary
            .groups
            .iter()
            .find(|g| g.name == name)
            .map(|g| g.nodes.iter().map(|n| n.name.clone()).collect())
            .ok_or_else(|| format!("代理组不存在: {name}")),
        None => {
            let mut seen = std::collections::HashSet::new();
            let mut out = Vec::new();
            for g in &summary.groups {
                // GLOBAL 组类型也是 Selector，但其 all 是全量节点合集，测它等于重复劳动
                if g.name == "GLOBAL" {
                    continue;
                }
                if !matches!(
                    g.group_type,
                    ProxyGroupType::Selector | ProxyGroupType::UrlTest
                ) {
                    continue;
                }
                for node in &g.nodes {
                    // 同一节点可能出现在多个组里，去重且保持首次出现顺序
                    if seen.insert(node.name.clone()) {
                        out.push(node.name.clone());
                    }
                }
            }
            Ok(out)
        }
    }
}

#[derive(Debug, Deserialize)]
struct VersionResponse {
    version: String,
    #[serde(default)]
    meta: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(server: &MockServer) -> MihomoClient {
        MihomoClient::new(server.uri(), None)
    }

    // ---- /proxies ----

    fn proxies_body() -> serde_json::Value {
        serde_json::json!({
            "proxies": {
                "节点选择": {
                    "name": "节点选择", "type": "Selector", "now": "香港 01",
                    "all": ["香港 01", "日本 01", "幽灵节点", "DIRECT"], "history": []
                },
                "自动优选": {
                    "name": "自动优选", "type": "URLTest", "now": "日本 01",
                    "all": ["香港 01", "日本 01"], "history": []
                },
                "新式组": {
                    "name": "新式组", "type": "Smart", "now": "DIRECT",
                    "all": ["DIRECT"], "history": []
                },
                "香港 01": {
                    "name": "香港 01", "type": "Vless", "udp": true,
                    // 末条带 meanDelay 时优先于 delay
                    "history": [
                        {"time": "2026-09-21T10:00:00Z", "delay": 120},
                        {"time": "2026-09-21T10:05:00Z", "meanDelay": 98, "delay": 999}
                    ]
                },
                "日本 01": {
                    "name": "日本 01", "type": "Hysteria2",
                    // 末条 delay=0 → 超时
                    "history": [{"time": "2026-09-21T10:05:00Z", "delay": 0}]
                },
                "DIRECT": { "name": "DIRECT", "type": "Direct", "history": [] }
            }
        })
    }

    #[tokio::test]
    async fn get_proxies_parses_groups_and_nodes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/proxies"))
            .respond_with(ResponseTemplate::new(200).set_body_json(proxies_body()))
            .mount(&server)
            .await;

        let summary = client(&server).get_proxies().await.unwrap();
        // 组按名字排序（码点序）：新式组 < 自动优选 < 节点选择
        let names: Vec<&str> = summary.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["新式组", "自动优选", "节点选择"]);

        let auto = &summary.groups[1];
        assert_eq!(auto.group_type, ProxyGroupType::UrlTest);
        assert_eq!(auto.now, "日本 01");
        assert_eq!(auto.nodes.len(), 2);

        let manual = &summary.groups[2];
        assert_eq!(manual.group_type, ProxyGroupType::Selector);
        assert_eq!(manual.nodes.len(), 4);
        // 有效延迟：meanDelay 优先
        assert_eq!(manual.nodes[0].name, "香港 01");
        assert_eq!(manual.nodes[0].node_type, "Vless");
        assert_eq!(manual.nodes[0].delay_ms, Some(98));
        assert_eq!(manual.nodes[0].delay_status, DelayStatus::Delayed);
        // delay=0 → 超时
        assert_eq!(manual.nodes[1].delay_ms, None);
        assert_eq!(manual.nodes[1].delay_status, DelayStatus::Timeout);
        // all 引用了响应里不存在的条目 → 占位 Untested，不丢整组
        assert_eq!(manual.nodes[2].name, "幽灵节点");
        assert_eq!(manual.nodes[2].delay_status, DelayStatus::Untested);
        // 空 history → 无记录
        assert_eq!(manual.nodes[3].name, "DIRECT");
        assert_eq!(manual.nodes[3].delay_status, DelayStatus::Untested);

        // 未识别的组类型降级为 Unknown
        assert_eq!(summary.groups[0].group_type, ProxyGroupType::Unknown);
    }

    #[tokio::test]
    async fn get_proxies_api_error_on_500() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let err = client(&server).get_proxies().await.unwrap_err();
        assert!(matches!(err, MihomoError::Api(_)));
    }

    #[tokio::test]
    async fn get_proxies_parse_error_when_proxies_key_missing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"x": 1})))
            .mount(&server)
            .await;

        let err = client(&server).get_proxies().await.unwrap_err();
        assert!(matches!(err, MihomoError::Parse(_)));
    }

    // ---- PUT /proxies/{group} ----

    #[tokio::test]
    async fn select_proxy_sends_name_body() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/proxies/%E8%8A%82%E7%82%B9%E9%80%89%E6%8B%A9")) // "节点选择" 编码后
            .and(body_json(serde_json::json!({ "name": "香港 01" })))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        client(&server)
            .select_proxy("节点选择", "香港 01")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn select_proxy_api_error_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(serde_json::json!({"message": "not found"})),
            )
            .mount(&server)
            .await;

        let err = client(&server)
            .select_proxy("无此组", "x")
            .await
            .unwrap_err();
        assert!(matches!(err, MihomoError::Api(_)));
    }

    // ---- GET /proxies/{name}/delay ----

    #[tokio::test]
    async fn test_delay_parses_delay() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/proxies/node-a/delay"))
            .and(query_param("timeout", "3000"))
            .and(query_param("url", "https://example.com/204"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"delay": 88})),
            )
            .mount(&server)
            .await;

        let r = client(&server)
            .test_delay("node-a", 3000, "https://example.com/204")
            .await
            .unwrap();
        assert_eq!(r.delay_ms, Some(88));
        assert_eq!(r.delay_status, DelayStatus::Delayed);
        assert_eq!(r.error, None);
    }

    #[tokio::test]
    async fn test_delay_408_maps_to_timeout_result() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(408)
                    .set_body_json(serde_json::json!({"message": "Proxy delay test timeout"})),
            )
            .mount(&server)
            .await;

        let r = client(&server)
            .test_delay("node-a", 1000, DEFAULT_TEST_URL)
            .await
            .unwrap();
        assert_eq!(r.delay_status, DelayStatus::Timeout);
        assert_eq!(r.delay_ms, None);
        assert_eq!(r.error, None);
    }

    #[tokio::test]
    async fn test_delay_zero_maps_to_timeout_result() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"delay": 0})))
            .mount(&server)
            .await;

        let r = client(&server)
            .test_delay("node-a", 1000, DEFAULT_TEST_URL)
            .await
            .unwrap();
        assert_eq!(r.delay_status, DelayStatus::Timeout);
    }

    #[tokio::test]
    async fn test_delay_api_error_on_404_and_500() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let err = client(&server)
            .test_delay("无此节点", 1000, DEFAULT_TEST_URL)
            .await
            .unwrap_err();
        assert!(matches!(err, MihomoError::Api(_)));
    }

    #[tokio::test]
    async fn test_delay_parse_error_when_delay_field_missing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"meanDelay": 1})),
            )
            .mount(&server)
            .await;

        let err = client(&server)
            .test_delay("node-a", 1000, DEFAULT_TEST_URL)
            .await
            .unwrap_err();
        assert!(matches!(err, MihomoError::Parse(_)));
    }

    #[tokio::test]
    async fn test_delays_batch_keeps_order_and_isolates_failures() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/proxies/node-a/delay"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"delay": 50})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/proxies/node-b/delay"))
            .respond_with(ResponseTemplate::new(408))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/proxies/node-c/delay"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let names = vec![
            "node-a".to_string(),
            "node-b".to_string(),
            "node-c".to_string(),
        ];
        let results = client(&server)
            .test_delays(&names, 1000, DEFAULT_TEST_URL)
            .await;
        assert_eq!(results.len(), 3);
        // 顺序与输入一致
        assert_eq!(results[0].name, "node-a");
        assert_eq!(results[0].delay_ms, Some(50));
        assert_eq!(results[1].name, "node-b");
        assert_eq!(results[1].delay_status, DelayStatus::Timeout);
        assert_eq!(results[1].error, None);
        // 单节点 500 只记入自身 error，不拖垮整批
        assert_eq!(results[2].name, "node-c");
        assert!(results[2].error.is_some());
    }

    // ---- PATCH /configs ----

    #[tokio::test]
    async fn set_mode_sends_mode_body() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/configs"))
            .and(body_json(serde_json::json!({ "mode": "global" })))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        client(&server).set_mode(RuleMode::Global).await.unwrap();
    }

    #[tokio::test]
    async fn set_mode_api_error_on_400() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;

        let err = client(&server).set_mode(RuleMode::Rule).await.unwrap_err();
        assert!(matches!(err, MihomoError::Api(_)));
    }

    // ---- GET /configs（读取当前规则模式） ----

    #[tokio::test]
    async fn get_mode_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/configs"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"mode": "global", "port": 7890})),
            )
            .mount(&server)
            .await;

        assert_eq!(client(&server).get_mode().await.unwrap(), RuleMode::Global);
    }

    #[tokio::test]
    async fn get_mode_parse_error_on_unknown_mode() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/configs"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"mode": "tun"})),
            )
            .mount(&server)
            .await;

        let err = client(&server).get_mode().await.unwrap_err();
        assert!(matches!(err, MihomoError::Parse(_)));
    }

    // ---- PUT /configs?force=（热重载） ----

    #[tokio::test]
    async fn reload_config_sends_force_query_and_empty_object_body() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/configs"))
            .and(query_param("force", "true"))
            .and(body_json(serde_json::json!({})))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        client(&server).reload_config(true).await.unwrap();
    }

    #[tokio::test]
    async fn reload_config_accepts_200_and_sends_force_false() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/configs"))
            .and(query_param("force", "false"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        client(&server).reload_config(false).await.unwrap();
    }

    #[tokio::test]
    async fn reload_config_api_error_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let err = client(&server).reload_config(true).await.unwrap_err();
        assert!(matches!(err, MihomoError::Api(_)));
    }

    #[tokio::test]
    async fn reload_config_unreachable_when_connection_refused() {
        let c = MihomoClient::new("http://127.0.0.1:1", None);
        let err = c.reload_config(false).await.unwrap_err();
        assert!(matches!(err, MihomoError::Unreachable(_)));
    }

    // ---- collect_test_node_names ----

    fn test_summary() -> ProxiesSummary {
        client_summary_fixture()
    }

    /// 与 get_proxies 测试同形状的摘要：Selector 组（节点选择）+ URLTest 组（自动优选）
    /// + Unknown 组（新式组），节点跨组重复（香港 01 / 日本 01 同在两组）
    fn client_summary_fixture() -> ProxiesSummary {
        fn node(name: &str) -> ProxyNode {
            ProxyNode {
                name: name.to_string(),
                node_type: String::new(),
                delay_ms: None,
                delay_status: DelayStatus::Untested,
            }
        }
        fn group(name: &str, ty: ProxyGroupType, nodes: &[&str]) -> ProxyGroup {
            ProxyGroup {
                name: name.to_string(),
                group_type: ty,
                now: String::new(),
                nodes: nodes.iter().map(|n| node(n)).collect(),
            }
        }
        ProxiesSummary {
            groups: vec![
                group("GLOBAL", ProxyGroupType::Selector, &["香港 01", "全局独有"]),
                group("节点选择", ProxyGroupType::Selector, &["香港 01", "DIRECT"]),
                group("自动优选", ProxyGroupType::UrlTest, &["香港 01", "日本 01"]),
                group("负载均衡", ProxyGroupType::LoadBalance, &["LB 节点"]),
            ],
        }
    }

    #[test]
    fn collect_names_all_groups_dedupes_and_skips_global_and_non_testable() {
        let names = collect_test_node_names(&test_summary(), None).unwrap();
        // GLOBAL 与 LoadBalance 组被排除；香港 01 跨组重复只保留首次出现
        assert_eq!(names, ["香港 01", "DIRECT", "日本 01"]);
    }

    #[test]
    fn collect_names_specific_group() {
        let names = collect_test_node_names(&test_summary(), Some("节点选择")).unwrap();
        assert_eq!(names, ["香港 01", "DIRECT"]);
    }

    #[test]
    fn collect_names_unknown_group_is_error() {
        let err = collect_test_node_names(&test_summary(), Some("无此组")).unwrap_err();
        assert!(err.contains("无此组"));
    }

    // ---- GET /version 与存活探测 ----

    #[tokio::test]
    async fn version_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/version"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"version": "v1.18.0", "meta": true})),
            )
            .mount(&server)
            .await;

        let c = client(&server);
        let v = c.version().await.unwrap();
        assert_eq!(v.version, "v1.18.0");
        assert_eq!(v.meta, Some(true));
        assert!(c.alive().await);
    }

    #[tokio::test]
    async fn unreachable_when_connection_refused() {
        // 端口 1 必拒连 → Unreachable（区分于 Api/Parse），存活探测为 false
        let c = MihomoClient::new("http://127.0.0.1:1", None);
        let err = c.version().await.unwrap_err();
        assert!(matches!(err, MihomoError::Unreachable(_)));
        assert!(!c.alive().await);
    }

    #[tokio::test]
    async fn unreachable_on_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;

        let c = MihomoClient::with_timeout(server.uri(), None, Duration::from_millis(100));
        let err = c.get_proxies().await.unwrap_err();
        assert!(matches!(err, MihomoError::Unreachable(_)));
    }

    #[test]
    fn client_holds_base_url() {
        let c = MihomoClient::new("http://127.0.0.1:9090", None);
        assert_eq!(c.base_url(), "http://127.0.0.1:9090");
    }

    // ---- external-controller secret 鉴权头 ----

    #[tokio::test]
    async fn bearer_auth_header_sent_when_secret_configured() {
        let server = MockServer::start().await;
        // mock 只响应带正确 Authorization 头的请求；其余一律 404（wiremock 无匹配默认行为）
        Mock::given(method("GET"))
            .and(path("/version"))
            .and(header("authorization", "Bearer test-secret-123"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"version": "v1.19.32", "meta": true})),
            )
            .mount(&server)
            .await;

        let authed = MihomoClient::new(server.uri(), Some("test-secret-123".to_string()));
        assert!(authed.alive().await);

        // 不带 secret → 无 Authorization 头 → 匹配不到 mock → 404 Api 错误
        let anon = client(&server);
        let err = anon.version().await.unwrap_err();
        assert!(matches!(err, MihomoError::Api(_)));
    }

    #[tokio::test]
    async fn bearer_auth_header_applies_to_write_endpoints() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/configs"))
            .and(header("authorization", "Bearer write-secret"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let c = MihomoClient::new(server.uri(), Some("write-secret".to_string()));
        c.set_mode(RuleMode::Global).await.unwrap();
    }
}
