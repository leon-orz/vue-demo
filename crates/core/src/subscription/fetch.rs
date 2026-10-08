//! 订阅链接拉取：URL 校验（仅 http/https）+ reqwest GET（超时 + UA 伪装）。
//!
//! UA 伪装成 Clash 系客户端：机场/订阅转换后端常按 UA 分发内容形态，
//! 用浏览器 UA 可能拿到 Base64 的 ss/ssr 列表而非 Clash 可用的内容。

use std::time::Duration;

use super::SubscriptionError;

/// 订阅拉取整体超时（机场高峰期可能较慢，15s 足够宽松）
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

/// UA 伪装（现网订阅转换惯例：伪装成 Clash 系客户端拿到 Clash/mihomo 配置）
const USER_AGENT: &str = "clash-verge/v2.4.2";

/// 校验订阅 URL：trim 后仅允许 http/https scheme 且必须有 host；
/// 返回规范化后的 URL 字符串（url crate 序列化形态）。
pub fn validate_subscription_url(url: &str) -> Result<String, SubscriptionError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(SubscriptionError::Link("订阅链接为空".to_string()));
    }
    let parsed = url::Url::parse(trimmed)
        .map_err(|e| SubscriptionError::Link(format!("订阅链接非法: {e}")))?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(SubscriptionError::Link(format!(
                "仅支持 http/https 订阅链接（当前: {scheme}）"
            )));
        }
    }
    if parsed.host_str().is_none() {
        return Err(SubscriptionError::Link("订阅链接缺主机名".to_string()));
    }
    Ok(parsed.to_string())
}

/// 拉取订阅内容：GET 指定 URL，超时 15s，UA 伪装成 Clash 系客户端。
/// 非 2xx → FetchHttp(status)；网络错误（连接失败/超时等）→ Fetch。
pub async fn fetch_subscription(url: &str) -> Result<String, SubscriptionError> {
    fetch_with_timeout(url, FETCH_TIMEOUT).await
}

/// 拉取实现（超时参数化便于测试超时路径；url 先过校验）
async fn fetch_with_timeout(url: &str, timeout: Duration) -> Result<String, SubscriptionError> {
    let url = validate_subscription_url(url)?;
    // 每次新建 client：超时与进程级共享 client（mihomo 10s 回环场景）不同，不值得复用
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| SubscriptionError::Fetch(format!("HTTP 客户端构建失败: {e}")))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| SubscriptionError::Fetch(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(SubscriptionError::FetchHttp(status.as_u16()));
    }
    resp.text()
        .await
        .map_err(|e| SubscriptionError::Fetch(format!("响应体读取失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ---- validate_subscription_url ----

    #[test]
    fn validate_accepts_http_and_https() {
        let url = validate_subscription_url("  https://sub.example.com/feed?token=abc  ").unwrap();
        assert_eq!(url, "https://sub.example.com/feed?token=abc");
        assert!(validate_subscription_url("http://192.168.1.1:8080/sub").is_ok());
    }

    #[test]
    fn validate_rejects_bad_urls() {
        // 空串 / 全空白
        assert!(matches!(
            validate_subscription_url("").unwrap_err(),
            SubscriptionError::Link(_)
        ));
        assert!(validate_subscription_url("   ").is_err());
        // 缺 scheme
        assert!(validate_subscription_url("sub.example.com/feed").is_err());
        // 非 http/https scheme
        assert!(validate_subscription_url("ftp://example.com/sub").is_err());
        // 缺 host（url crate 对 http/https 空 host 直接判 parse 失败）
        assert!(validate_subscription_url("https://").is_err());
    }

    // ---- fetch_subscription ----

    #[tokio::test]
    async fn fetch_returns_body_on_200_with_disguised_ua() {
        let server = MockServer::start().await;
        // mock 只响应带伪装 UA 的请求，顺带验证 UA 头确实发出
        Mock::given(method("GET"))
            .and(path("/sub"))
            .and(header("user-agent", USER_AGENT))
            .respond_with(ResponseTemplate::new(200).set_body_string("proxies: []"))
            .mount(&server)
            .await;

        let body = fetch_subscription(&format!("{}/sub", server.uri()))
            .await
            .unwrap();
        assert_eq!(body, "proxies: []");
    }

    #[tokio::test]
    async fn fetch_maps_non_2xx_to_fetch_error_with_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = fetch_subscription(&server.uri()).await.unwrap_err();
        assert!(matches!(err, SubscriptionError::FetchHttp(404)));
        assert!(err.to_string().contains("404"));

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let err = fetch_subscription(&server.uri()).await.unwrap_err();
        assert!(matches!(err, SubscriptionError::FetchHttp(401)));
        assert!(err.to_string().contains("401"));
    }

    #[tokio::test]
    async fn fetch_maps_timeout_to_fetch_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
            .mount(&server)
            .await;

        // 注入 100ms 短超时，避免测试真的等 15s
        let err = fetch_with_timeout(&server.uri(), Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(matches!(err, SubscriptionError::Fetch(_)));
    }

    #[tokio::test]
    async fn fetch_rejects_invalid_url_before_request() {
        let err = fetch_subscription("ftp://example.com/sub")
            .await
            .unwrap_err();
        assert!(matches!(err, SubscriptionError::Link(_)));
    }
}
