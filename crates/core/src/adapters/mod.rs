//! 平台适配器统一 trait。加新平台 = 加一个文件 + 注册一行。
//! S1-2 落地 kimi（开放平台余额）；new-api 网关适配器属阶段二 M2，届时新增 newapi_admin.rs。
//! 注：第三方中转平台统计已明确不做（《01-需求文档》第 7 章），不会有对应适配器。

pub mod kimi;

use thiserror::Error;

use crate::models::{Credential, UsageSnapshot};

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

/// 各平台直连适配器。实现差异（认证方式、quota 换算）隔离在各自文件内。
// 项目内部 trait，不涉及跨 crate 的 auto trait 兼容问题，允许 async fn
#[allow(async_fn_in_trait)]
pub trait UsageAdapter: Send + Sync {
    /// 平台类型标识，对应 platform.type 字段（如 "newapi" / "kimi" / "gateway"）
    fn platform_type(&self) -> &'static str;

    async fn fetch(&self, base_url: &str, cred: &Credential)
        -> Result<UsageSnapshot, AdapterError>;

    // TODO(S2-3)：quota -> 金额的换算钩子（new-api 默认 $1 = 500,000 quota，可被平台配置覆盖）
}
