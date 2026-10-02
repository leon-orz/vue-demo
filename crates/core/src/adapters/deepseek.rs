//! DeepSeek 官方余额适配器。
//! GET {api_base}/user/balance，Bearer key 认证（官方文档化接口）。
//! 响应 balance_infos 按币种分项，金额字段官方返回字符串（如 "110.00"），解析时容忍数字与字符串两种形态。
//! 按量余额无总额概念：total/used 留空，remaining 为各币种 total_balance 之和，分项明细放 detail_json。

use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

use super::{AdapterError, UsageAdapter};
use crate::models::{Credential, Platform, QuotaSnapshot};

pub const DEFAULT_API_BASE: &str = "https://api.deepseek.com";
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

/// 宽松数值解析：官方文档中金额为字符串（"110.00"），容忍直接给数字的情况
fn de_num<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    json_to_f64(&value).map_err(serde::de::Error::custom)
}

fn de_num_opt<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => json_to_f64(&v).map(Some).map_err(serde::de::Error::custom),
    }
}

fn json_to_f64(value: &serde_json::Value) -> Result<f64, String> {
    match value {
        serde_json::Value::Number(n) => n.as_f64().ok_or_else(|| "数值超出 f64 范围".to_string()),
        serde_json::Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("金额字符串无法解析: {s}")),
        other => Err(format!("金额应为数字或数字字符串，实际为 {other}")),
    }
}

/// 余额接口响应（官方格式）：is_available 表示余额是否充足，balance_infos 按币种分项
#[derive(Debug, Deserialize)]
struct BalanceResponse {
    #[serde(default)]
    is_available: Option<bool>,
    balance_infos: Option<Vec<BalanceInfo>>,
}

#[derive(Debug, Deserialize)]
struct BalanceInfo {
    #[serde(default = "default_currency")]
    currency: String,
    #[serde(deserialize_with = "de_num")]
    total_balance: f64,
    #[serde(default, deserialize_with = "de_num_opt")]
    granted_balance: Option<f64>,
    #[serde(default, deserialize_with = "de_num_opt")]
    topped_up_balance: Option<f64>,
}

fn default_currency() -> String {
    "unknown".to_string()
}

/// 单个币种的余额分项
#[derive(Debug, Clone, PartialEq)]
pub struct DeepSeekBalanceInfo {
    pub currency: String,
    pub total: f64,
    pub granted: Option<f64>,
    pub topped_up: Option<f64>,
}

/// DeepSeek 余额查询结果。remaining 为各币种 total_balance 之和。
#[derive(Debug, Clone)]
pub struct DeepSeekBalance {
    pub remaining: f64,
    /// 官方字段：余额是否充足（true 表示还有余额可用）
    pub is_available: Option<bool>,
    pub infos: Vec<DeepSeekBalanceInfo>,
    /// RFC 3339 拉取时间
    pub fetched_at: String,
}

pub struct DeepSeekAdapter {
    client: reqwest::Client,
}

impl Default for DeepSeekAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DeepSeekAdapter {
    pub fn new() -> Self {
        Self {
            client: shared_client(),
        }
    }

    /// 自定义超时的独立客户端（主要用于测试超时路径）
    pub fn with_timeout(timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client 构建失败");
        Self { client }
    }

    /// 拉取余额。base_url 允许带或不带尾斜杠。
    pub async fn fetch_balance(
        &self,
        base_url: &str,
        api_key: &str,
    ) -> Result<DeepSeekBalance, AdapterError> {
        let url = format!("{}/user/balance", base_url.trim_end_matches('/'));
        let resp = self
            .client
            .get(&url)
            .bearer_auth(api_key)
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AdapterError::Unauthorized(format!(
                "余额接口返回 {status}，请检查 API key"
            )));
        }
        if !status.is_success() {
            return Err(AdapterError::Api(format!("余额接口返回 {status}")));
        }
        let body: BalanceResponse = resp
            .json()
            .await
            .map_err(|e| AdapterError::Parse(e.to_string()))?;
        let infos = body
            .balance_infos
            .ok_or_else(|| AdapterError::Parse("响应缺少 balance_infos 字段".to_string()))?;
        let infos: Vec<DeepSeekBalanceInfo> = infos
            .into_iter()
            .map(|i| DeepSeekBalanceInfo {
                currency: i.currency,
                total: i.total_balance,
                granted: i.granted_balance,
                topped_up: i.topped_up_balance,
            })
            .collect();
        let remaining = infos.iter().map(|i| i.total).sum();
        Ok(DeepSeekBalance {
            remaining,
            is_available: body.is_available,
            infos,
            fetched_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
    }
}

impl UsageAdapter for DeepSeekAdapter {
    fn platform_type(&self) -> &'static str {
        "deepseek"
    }

    async fn fetch(
        &self,
        base_url: &str,
        cred: &Credential,
    ) -> Result<QuotaSnapshot, AdapterError> {
        let Credential::ApiKey(key) = cred else {
            return Err(AdapterError::Parse(
                "DeepSeek 仅支持 ApiKey 凭据".to_string(),
            ));
        };
        let balance = self.fetch_balance(base_url, key).await?;
        // 按量余额无总额概念：只填 remaining（各币种合计），币种分项进 detail_json
        Ok(QuotaSnapshot {
            platform: Platform::Deepseek,
            total: None,
            used: None,
            remaining: Some(balance.remaining),
            detail_json: Some(
                serde_json::json!({
                    "is_available": balance.is_available,
                    "balances": balance.infos.iter().map(|i| serde_json::json!({
                        "currency": i.currency,
                        "total": i.total,
                        "granted": i.granted,
                        "topped_up": i.topped_up,
                    })).collect::<Vec<_>>(),
                })
                .to_string(),
            ),
            fetched_at: balance.fetched_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ok_body() -> serde_json::Value {
        // 官方格式：金额为字符串，按币种分项
        serde_json::json!({
            "is_available": true,
            "balance_infos": [
                {
                    "currency": "CNY",
                    "total_balance": "110.00",
                    "granted_balance": "10.00",
                    "topped_up_balance": "100.00"
                },
                {
                    "currency": "USD",
                    "total_balance": "5.50",
                    "granted_balance": "0.00",
                    "topped_up_balance": "5.50"
                }
            ]
        })
    }

    #[tokio::test]
    async fn parses_balance_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/balance"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;

        let balance = DeepSeekAdapter::new()
            .fetch_balance(&server.uri(), "test-key")
            .await
            .unwrap();
        assert_eq!(balance.infos.len(), 2);
        assert_eq!(balance.infos[0].currency, "CNY");
        assert_eq!(balance.infos[0].total, 110.0);
        assert_eq!(balance.infos[0].granted, Some(10.0));
        assert_eq!(balance.infos[0].topped_up, Some(100.0));
        // 各币种合计
        assert_eq!(balance.remaining, 115.5);
        assert_eq!(balance.is_available, Some(true));
        assert!(!balance.fetched_at.is_empty());
    }

    #[tokio::test]
    async fn tolerates_numeric_amounts() {
        // 宽松解析：金额为数字而非字符串时也接受
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "is_available": false,
                "balance_infos": [{"currency": "CNY", "total_balance": 0.0}]
            })))
            .mount(&server)
            .await;

        let balance = DeepSeekAdapter::new()
            .fetch_balance(&server.uri(), "k")
            .await
            .unwrap();
        assert_eq!(balance.remaining, 0.0);
        assert_eq!(balance.infos[0].granted, None);
    }

    #[tokio::test]
    async fn maps_to_quota_snapshot_via_trait() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;

        let adapter = DeepSeekAdapter::new();
        let snap = adapter
            .fetch(&server.uri(), &Credential::ApiKey("k".into()))
            .await
            .unwrap();
        assert_eq!(adapter.platform_type(), "deepseek");
        assert_eq!(snap.platform, Platform::Deepseek);
        assert_eq!(snap.total, None);
        assert_eq!(snap.used, None);
        assert_eq!(snap.remaining, Some(115.5));
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert_eq!(detail["is_available"], true);
        assert_eq!(detail["balances"][0]["currency"], "CNY");
        assert_eq!(detail["balances"][1]["total"], 5.5);
    }

    #[tokio::test]
    async fn unauthorized_on_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let err = DeepSeekAdapter::new()
            .fetch_balance(&server.uri(), "bad-key")
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn network_error_on_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;

        let err = DeepSeekAdapter::with_timeout(Duration::from_millis(100))
            .fetch_balance(&server.uri(), "k")
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Network(_)));
    }

    #[tokio::test]
    async fn parse_error_on_missing_balance_infos() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "is_available": true
            })))
            .mount(&server)
            .await;

        let err = DeepSeekAdapter::new()
            .fetch_balance(&server.uri(), "k")
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Parse(_)));
    }

    #[tokio::test]
    async fn parse_error_on_unparseable_amount() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "balance_infos": [{"currency": "CNY", "total_balance": "abc"}]
            })))
            .mount(&server)
            .await;

        let err = DeepSeekAdapter::new()
            .fetch_balance(&server.uri(), "k")
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Parse(_)));
    }

    #[tokio::test]
    async fn cookie_credential_rejected() {
        let err = DeepSeekAdapter::new()
            .fetch("http://127.0.0.1:1", &Credential::Cookie("c".into()))
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Parse(_)));
    }
}
