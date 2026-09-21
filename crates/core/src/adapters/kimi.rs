//! Kimi 开放平台余额适配器（M1-10）。
//! GET {api_base}/v1/users/me/balance，Bearer key 认证；key 由调用方从 keystore 取出传入，
//! 本适配器不接触存储层。字段：available_balance / voucher_balance / cash_balance（人民币元）。

use std::time::Duration;

use serde::Deserialize;

use super::{AdapterError, UsageAdapter};
use crate::models::{Credential, Currency, KimiBalance, UsageSnapshot};

pub const DEFAULT_API_BASE: &str = "https://api.moonshot.cn";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

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
        Self::with_timeout(DEFAULT_TIMEOUT)
    }

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
}

impl UsageAdapter for KimiAdapter {
    fn platform_type(&self) -> &'static str {
        "kimi"
    }

    async fn fetch(
        &self,
        base_url: &str,
        cred: &Credential,
    ) -> Result<UsageSnapshot, AdapterError> {
        let Credential::ApiKey(key) = cred else {
            return Err(AdapterError::Parse(
                "Kimi 开放平台仅支持 ApiKey 凭据".to_string(),
            ));
        };
        let balance = self.fetch_balance(base_url, key).await?;
        // 统一快照只承载总额：available 即账户可用余额；分项由 fetch_balance 提供
        Ok(UsageSnapshot {
            platform_id: 0,
            balance: balance.available,
            used: 0.0,
            currency: Currency::Cny,
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
        assert_eq!(snap.balance, 10.5);
        assert_eq!(snap.currency, Currency::Cny);
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
}
