//! 平台适配器。当前只有 kimi（开放平台余额 + coding usages），生产路径直接调
//! `KimiAdapter` 的 inherent 方法（见 usage::quota）。
//! 注：第三方中转平台统计已明确不做（《01-需求文档》第 7 章），不会有对应适配器。

pub mod kimi;

use thiserror::Error;

use crate::models::CommandError;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("网络请求失败: {0}")]
    Network(String),
    #[error("认证失败(401/403): {0}")]
    Unauthorized(String),
    #[error("接口返回错误: {0}")]
    Api(String),
    #[error("响应解析失败: {0}")]
    Parse(String),
}

impl From<AdapterError> for CommandError {
    fn from(e: AdapterError) -> Self {
        match e {
            AdapterError::Unauthorized(_) => CommandError::unauthorized(e.to_string()),
            AdapterError::Network(_) => CommandError::network(e.to_string()),
            AdapterError::Api(_) | AdapterError::Parse(_) => CommandError::internal(e.to_string()),
        }
    }
}
