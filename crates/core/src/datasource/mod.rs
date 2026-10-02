//! 数据获取层抽象（见《02-开发计划》1.5）：DirectSource 直连各平台，
//! HubSource 从中枢拉取（阶段二）。UI 层面向 trait 编程，切换数据源零改动。

use thiserror::Error;

use crate::models::QuotaSnapshot;

#[derive(Debug, Error)]
pub enum DataSourceError {
    #[error("适配器错误: {0}")]
    Adapter(#[from] crate::adapters::AdapterError),
    #[error("中枢不可达: {0}")]
    HubUnreachable(String),
    #[error("功能尚未实现: {0}")]
    NotImplemented(&'static str),
}

#[allow(async_fn_in_trait)] // 项目内部 trait，同上
pub trait DataSource: Send + Sync {
    async fn fetch_all_usage(&self) -> Result<Vec<QuotaSnapshot>, DataSourceError>;
}

/// 阶段一默认：客户端直连各平台
pub struct DirectSource;

impl DataSource for DirectSource {
    async fn fetch_all_usage(&self) -> Result<Vec<QuotaSnapshot>, DataSourceError> {
        // TODO(S1-4)：遍历启用的 platform_account，调度对应适配器并发拉取。
        // 占位期间显式报错而非返回空 vec，避免把"未实现"误读为"无用量"
        Err(DataSourceError::NotImplemented(
            "DirectSource 聚合调度（S1-4，随多平台配额适配器落地）",
        ))
    }
}

/// 阶段二：从 hub 拉取聚合快照；连续不可达时由上层降级回 DirectSource
pub struct HubSource {
    pub hub_base_url: String,
}

impl DataSource for HubSource {
    async fn fetch_all_usage(&self) -> Result<Vec<QuotaSnapshot>, DataSourceError> {
        // TODO(S2-2)：GET {hub_base_url}/api/v1/usage
        Err(DataSourceError::HubUnreachable(self.hub_base_url.clone()))
    }
}
