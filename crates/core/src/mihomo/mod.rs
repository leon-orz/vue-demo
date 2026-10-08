//! mihomo RESTful API（默认 9090）薄封装（S1-3 落地）。
//! hub 不需要本模块：魔法工具是客户端本地功能，中枢不可达时也必须可用。

mod client;

pub use client::{
    collect_test_node_names, MihomoClient, DEFAULT_DELAY_TIMEOUT_MS, DEFAULT_TEST_URL,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MihomoError {
    /// 连接失败或请求超时（sidecar 未拉起 / 端口被占 / 端口不对）
    #[error("mihomo 不可达: {0}")]
    Unreachable(String),
    /// 收到响应但状态码非 2xx（组/节点不存在、模式非法等）
    #[error("mihomo 返回错误: {0}")]
    Api(String),
    /// 响应体解析失败（内核版本不兼容、字段缺失等）
    #[error("mihomo 响应解析失败: {0}")]
    Parse(String),
}

/// MihomoError → CommandError：不可达归 network，其余归 internal
impl From<MihomoError> for crate::models::CommandError {
    fn from(e: MihomoError) -> Self {
        use crate::models::CommandError;
        match e {
            MihomoError::Unreachable(m) => CommandError::network(m),
            MihomoError::Api(m) | MihomoError::Parse(m) => CommandError::internal(m),
        }
    }
}
