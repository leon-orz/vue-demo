//! 平台适配器统一 trait。加新平台 = 加一个文件 + 注册一行。
//! S1-2 落地 kimi（开放平台余额）；S1-4 新增 deepseek（官方 /user/balance）与 mimo（手录总额+本地推算，无网络请求）。
//! 注：第三方中转平台统计已明确不做（《01-需求文档》第 7 章），不会有对应适配器。

pub mod deepseek;
pub mod kimi;
pub mod mimo;

use thiserror::Error;

use crate::models::{CommandError, Credential, QuotaSnapshot};

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
    /// 配额不可用（如 MiMo 未手录套餐总额、账户停用、计费周期不支持）
    #[error("配额不可用: {0}")]
    Unavailable(String),
    #[error("数据库错误: {0}")]
    Db(String),
}

impl From<sqlx::Error> for AdapterError {
    fn from(e: sqlx::Error) -> Self {
        AdapterError::Db(e.to_string())
    }
}

impl From<AdapterError> for CommandError {
    fn from(e: AdapterError) -> Self {
        match e {
            AdapterError::Unauthorized(_) => CommandError::unauthorized(e.to_string()),
            AdapterError::Network(_) => CommandError::network(e.to_string()),
            AdapterError::Unavailable(_) => CommandError::unavailable(e.to_string()),
            AdapterError::Api(_) | AdapterError::Parse(_) | AdapterError::Db(_) => {
                CommandError::internal(e.to_string())
            }
        }
    }
}

/// 各平台直连适配器。实现差异（认证方式、quota 换算）隔离在各自文件内。
/// 注：mimo 为纯本地推算（读 platform_account + usage_event，无网络请求），
/// 不实现本 trait，入口为 MimoAdapter::snapshot(pool)。
// 项目内部 trait，不涉及跨 crate 的 auto trait 兼容问题，允许 async fn
#[allow(async_fn_in_trait)]
pub trait UsageAdapter: Send + Sync {
    /// 平台标识，对应 platform_account.platform / quota_snapshot.platform 列
    /// （"kimi" / "deepseek" / "mimo"，见 models::Platform）
    fn platform_type(&self) -> &'static str;

    /// 拉取配额/余额并归一化为 QuotaSnapshot（统一落 quota_snapshot 表）
    async fn fetch(&self, base_url: &str, cred: &Credential)
        -> Result<QuotaSnapshot, AdapterError>;
}
