//! mihomo RESTful API（默认 9090）薄封装（S1-3 落地）。
//! hub 不需要本模块：魔法工具是客户端本地功能，中枢不可达时也必须可用。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MihomoError {
    #[error("mihomo 不可达: {0}")]
    Unreachable(String),
    #[error("mihomo 返回错误: {0}")]
    Api(String),
}

pub struct MihomoClient {
    base_url: String,
}

impl MihomoClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // TODO(S1-3)：
    // - GET  /proxies                 代理组与节点摘要
    // - PUT  /proxies/{group}         切换节点
    // - GET  /proxies/{name}/delay    延迟测试
    // - PATCH /configs                规则模式切换
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_holds_base_url() {
        let client = MihomoClient::new("http://127.0.0.1:9090");
        assert_eq!(client.base_url(), "http://127.0.0.1:9090");
    }
}
