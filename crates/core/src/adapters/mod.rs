//! 平台适配器统一 trait。加新平台 = 加一个文件 + 注册一行。
//! S1-2 落地 kimi（开放平台余额）；S1-4 新增 deepseek（官方 /user/balance）与 mimo（手录总额+本地推算，无网络请求）。
//! 注：第三方中转平台统计已明确不做（《01-需求文档》第 7 章），不会有对应适配器。

pub mod kimi;

use thiserror::Error;

use crate::models::{CommandError, Credential, UsageSnapshot};

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

/// 各平台直连适配器。实现差异（认证方式、quota 换算）隔离在各自文件内。
// 项目内部 trait，不涉及跨 crate 的 auto trait 兼容问题，允许 async fn
#[allow(async_fn_in_trait)]
pub trait UsageAdapter: Send + Sync {
    /// 平台类型标识，对应 platform.type 字段（如 "kimi" / "deepseek" / "mimo"）
    fn platform_type(&self) -> &'static str;

    async fn fetch(&self, base_url: &str, cred: &Credential)
        -> Result<UsageSnapshot, AdapterError>;
}
