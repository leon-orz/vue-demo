//! Kimi 开放平台余额适配器（M1-10）。
//! GET {api_base}/v1/users/me/balance，Bearer key 认证；key 由调用方从 keystore 取出传入，
//! 本适配器不接触存储层。字段：available_balance / voucher_balance / cash_balance（人民币元）。
//! 另含 Kimi Code 套餐用量接口 GET {coding_api_base}/coding/v1/usages（非官方、best-effort，
//! 格式为推测：5h 滚动窗口 + 月度池余量；失败/字段缺失降级为明确错误，不 panic）。

use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

use super::{AdapterError, UsageAdapter};
use crate::models::{Credential, Platform, QuotaSnapshot};

pub const DEFAULT_API_BASE: &str = "https://api.moonshot.cn";
/// Kimi Code 套餐用量接口的默认 base（区别于开放平台余额的 api.moonshot.cn）
pub const DEFAULT_CODING_API_BASE: &str = "https://api.kimi.com";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Kimi 开放平台余额（内部类型，单位人民币元）。
/// 对外契约是 QuotaSnapshot（quota_snapshot 表），本结构只在适配器内部承载接口字段；
/// 原为 models::KimiBalance（旧 kimi_balance_* 命令的返回类型），S1-4 收尾时随旧命令下线收回本模块。
#[derive(Debug, Clone, PartialEq)]
pub struct KimiBalance {
    pub available: f64,
    pub voucher: f64,
    pub cash: f64,
    /// RFC 3339 拉取时间
    pub fetched_at: String,
}

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

/// 余额接口响应（外层 code/status 字段忽略，只取 data）
#[derive(Debug, Deserialize)]
struct BalanceResponse {
    data: Option<BalanceData>,
}

#[derive(Debug, Deserialize)]
struct BalanceData {
    available_balance: f64,
    voucher_balance: f64,
    cash_balance: f64,
}

// ---- /coding/v1/usages（非官方接口，best-effort） ----
// 注意：响应格式未文档化，以下结构为推测。字段全部宽松解析（Option + serde default），
// 单字段缺失降级为 None；仅当整个 data 缺失时才报 Parse。真实响应格式确认后需同步修正
// 结构与下方测试 fixture。

#[derive(Debug, Deserialize)]
struct UsagesResponse {
    data: Option<UsagesData>,
}

#[derive(Debug, Default, Deserialize)]
struct UsagesData {
    /// 5 小时滚动窗口用量（推测字段）
    #[serde(default)]
    window: Option<UsageWindow>,
    /// 月度配额池（推测字段）
    #[serde(default)]
    month: Option<UsageMonth>,
}

#[derive(Debug, Default, Deserialize)]
struct UsageWindow {
    used_percent: Option<f64>,
    remaining_percent: Option<f64>,
    /// 窗口重置时间（推测为 RFC 3339 字符串）
    reset_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct UsageMonth {
    total: Option<f64>,
    used: Option<f64>,
    remaining: Option<f64>,
}

/// Kimi Code 套餐用量（5h 滚动窗口 + 月度池）。字段均可空：
/// 接口未文档化、best-effort，解析不到的字段保持 None 而非报错。
#[derive(Debug, Clone, Default)]
pub struct KimiCodingUsages {
    pub window_used_percent: Option<f64>,
    pub window_remaining_percent: Option<f64>,
    pub window_reset_at: Option<String>,
    pub month_total: Option<f64>,
    pub month_used: Option<f64>,
    pub month_remaining: Option<f64>,
    /// RFC 3339 拉取时间
    pub fetched_at: String,
}

pub struct KimiAdapter {
    client: reqwest::Client,
}

impl Default for KimiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl KimiAdapter {
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
    ) -> Result<KimiBalance, AdapterError> {
        let url = format!("{}/v1/users/me/balance", base_url.trim_end_matches('/'));
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
        let data = body
            .data
            .ok_or_else(|| AdapterError::Parse("响应缺少 data 字段".to_string()))?;
        Ok(KimiBalance {
            available: data.available_balance,
            voucher: data.voucher_balance,
            cash: data.cash_balance,
            fetched_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
    }

    /// 拉取 Kimi Code 套餐用量（5h 滚动窗口 + 月度池）。base_url 允许带或不带尾斜杠。
    /// 非官方接口、best-effort：网络/认证失败照常报错，字段缺失宽松降级为 None，
    /// 仅整个 data 缺失时报 Parse——调用方据此把卡片降级为"不可用"。
    pub async fn fetch_coding_usages(
        &self,
        base_url: &str,
        api_key: &str,
    ) -> Result<KimiCodingUsages, AdapterError> {
        let url = format!("{}/coding/v1/usages", base_url.trim_end_matches('/'));
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
                "套餐用量接口返回 {status}，请检查 API key"
            )));
        }
        if !status.is_success() {
            return Err(AdapterError::Api(format!("套餐用量接口返回 {status}")));
        }
        let body: UsagesResponse = resp
            .json()
            .await
            .map_err(|e| AdapterError::Parse(e.to_string()))?;
        let data = body
            .data
            .ok_or_else(|| AdapterError::Parse("响应缺少 data 字段".to_string()))?;
        let (window, month) = (
            data.window.unwrap_or_default(),
            data.month.unwrap_or_default(),
        );
        Ok(KimiCodingUsages {
            window_used_percent: window.used_percent,
            window_remaining_percent: window.remaining_percent,
            window_reset_at: window.reset_at,
            month_total: month.total,
            month_used: month.used,
            month_remaining: month.remaining,
            fetched_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
    }

    /// 套餐用量 → QuotaSnapshot（统一落 quota_snapshot 表）：
    /// 月度池填 total/used/remaining（均可空），5h 窗口明细进 detail_json。
    pub async fn fetch_coding_usages_snapshot(
        &self,
        base_url: &str,
        cred: &Credential,
    ) -> Result<QuotaSnapshot, AdapterError> {
        let Credential::ApiKey(key) = cred else {
            return Err(AdapterError::Parse(
                "Kimi 套餐用量仅支持 ApiKey 凭据".to_string(),
            ));
        };
        let usages = self.fetch_coding_usages(base_url, key).await?;
        Ok(QuotaSnapshot {
            platform: Platform::Kimi,
            total: usages.month_total,
            used: usages.month_used,
            remaining: usages.month_remaining,
            detail_json: Some(
                serde_json::json!({
                    "source": "coding_usages",
                    "window_used_percent": usages.window_used_percent,
                    "window_remaining_percent": usages.window_remaining_percent,
                    "window_reset_at": usages.window_reset_at,
                })
                .to_string(),
            ),
            fetched_at: usages.fetched_at,
        })
    }
}

impl UsageAdapter for KimiAdapter {
    fn platform_type(&self) -> &'static str {
        "kimi"
    }

    async fn fetch(
        &self,
        base_url: &str,
        cred: &Credential,
    ) -> Result<QuotaSnapshot, AdapterError> {
        let Credential::ApiKey(key) = cred else {
            return Err(AdapterError::Parse(
                "Kimi 开放平台仅支持 ApiKey 凭据".to_string(),
            ));
        };
        let balance = self.fetch_balance(base_url, key).await?;
        // 按量余额无总额概念：只填 remaining=available，voucher/cash 分项与币种进 detail_json
        Ok(QuotaSnapshot {
            platform: Platform::Kimi,
            total: None,
            used: None,
            remaining: Some(balance.available),
            detail_json: Some(
                serde_json::json!({
                    "voucher": balance.voucher,
                    "cash": balance.cash,
                    "currency": "cny",
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
        serde_json::json!({
            "code": 0,
            "status": true,
            "data": {
                "available_balance": 10.5,
                "voucher_balance": 2.0,
                "cash_balance": 8.5
            }
        })
    }

    #[tokio::test]
    async fn parses_balance_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/users/me/balance"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;

        let adapter = KimiAdapter::new();
        let balance = adapter
            .fetch_balance(&server.uri(), "test-key")
            .await
            .unwrap();
        assert_eq!(balance.available, 10.5);
        assert_eq!(balance.voucher, 2.0);
        assert_eq!(balance.cash, 8.5);
        assert!(!balance.fetched_at.is_empty());
    }

    #[tokio::test]
    async fn maps_to_usage_snapshot_via_trait() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;

        let adapter = KimiAdapter::new();
        let snap = adapter
            .fetch(&server.uri(), &Credential::ApiKey("k".into()))
            .await
            .unwrap();
        assert_eq!(adapter.platform_type(), "kimi");
        assert_eq!(snap.platform, Platform::Kimi);
        assert_eq!(snap.remaining, Some(10.5));
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert_eq!(detail["voucher"], 2.0);
        assert_eq!(detail["currency"], "cny");
    }

    #[tokio::test]
    async fn unauthorized_on_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let err = KimiAdapter::new()
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

        let err = KimiAdapter::with_timeout(Duration::from_millis(100))
            .fetch_balance(&server.uri(), "k")
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Network(_)));
    }

    #[tokio::test]
    async fn parse_error_on_missing_fields() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                // 缺 cash_balance
                "data": {"available_balance": 1.0, "voucher_balance": 0.0}
            })))
            .mount(&server)
            .await;

        let err = KimiAdapter::new()
            .fetch_balance(&server.uri(), "k")
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Parse(_)));
    }

    #[tokio::test]
    async fn parse_error_when_data_absent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"code": -1})))
            .mount(&server)
            .await;

        let err = KimiAdapter::new()
            .fetch_balance(&server.uri(), "k")
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Parse(_)));
    }

    #[tokio::test]
    async fn cookie_credential_rejected() {
        let err = KimiAdapter::new()
            .fetch("http://127.0.0.1:1", &Credential::Cookie("c".into()))
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Parse(_)));
    }

    // ---- /coding/v1/usages（非官方接口，响应格式为推测，由以下 fixture 锁定） ----

    fn usages_body() -> serde_json::Value {
        // 推测格式：data.window 为 5h 滚动窗口，data.month 为月度池
        serde_json::json!({
            "code": 0,
            "status": true,
            "data": {
                "window": {
                    "used_percent": 12.5,
                    "remaining_percent": 87.5,
                    "reset_at": "2026-09-21T15:00:00Z"
                },
                "month": {
                    "total": 1000.0,
                    "used": 300.0,
                    "remaining": 700.0
                }
            }
        })
    }

    #[tokio::test]
    async fn parses_coding_usages_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/coding/v1/usages"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(usages_body()))
            .mount(&server)
            .await;

        let usages = KimiAdapter::new()
            .fetch_coding_usages(&server.uri(), "test-key")
            .await
            .unwrap();
        assert_eq!(usages.window_used_percent, Some(12.5));
        assert_eq!(usages.window_remaining_percent, Some(87.5));
        assert_eq!(
            usages.window_reset_at.as_deref(),
            Some("2026-09-21T15:00:00Z")
        );
        assert_eq!(usages.month_total, Some(1000.0));
        assert_eq!(usages.month_remaining, Some(700.0));
        assert!(!usages.fetched_at.is_empty());
    }

    #[tokio::test]
    async fn coding_usages_snapshot_via_trait_shape() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(usages_body()))
            .mount(&server)
            .await;

        let snap = KimiAdapter::new()
            .fetch_coding_usages_snapshot(&server.uri(), &Credential::ApiKey("k".into()))
            .await
            .unwrap();
        assert_eq!(snap.platform, Platform::Kimi);
        assert_eq!(snap.total, Some(1000.0));
        assert_eq!(snap.used, Some(300.0));
        assert_eq!(snap.remaining, Some(700.0));
        let detail: serde_json::Value =
            serde_json::from_str(snap.detail_json.as_deref().unwrap()).unwrap();
        assert_eq!(detail["source"], "coding_usages");
        assert_eq!(detail["window_used_percent"], 12.5);
    }

    #[tokio::test]
    async fn coding_usages_unauthorized_on_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let err = KimiAdapter::new()
            .fetch_coding_usages(&server.uri(), "bad-key")
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn coding_usages_missing_fields_degrade_to_none() {
        // best-effort 宽松解析：window 整体缺失 → 对应字段为 None，不报错
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"month": {"total": 1000.0, "remaining": 900.0}}
            })))
            .mount(&server)
            .await;

        let usages = KimiAdapter::new()
            .fetch_coding_usages(&server.uri(), "k")
            .await
            .unwrap();
        assert_eq!(usages.window_used_percent, None);
        assert_eq!(usages.month_total, Some(1000.0));
        assert_eq!(usages.month_used, None);
    }

    #[tokio::test]
    async fn coding_usages_parse_error_when_data_absent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"code": -1})))
            .mount(&server)
            .await;

        let err = KimiAdapter::new()
            .fetch_coding_usages(&server.uri(), "k")
            .await
            .unwrap_err();
        assert!(matches!(err, AdapterError::Parse(_)));
    }
}
