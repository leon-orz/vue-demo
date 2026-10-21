//! 节点导入（M5-07）对外模型：订阅/手动节点聚合状态。
//! 解析与拉取逻辑见 crate::subscription；持久化键见 crate::proxy_setting。
//! TS 类型由 ts-rs 生成到 packages/core-types/bindings/，禁止手改生成物。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::subscription::NodeFailure;

/// 订阅/手动节点导入的聚合状态（前端订阅卡片展示用）。
/// 各字段独立降级：未配置订阅、从未更新成功都属正常态而非错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct SubscriptionInfo {
    /// 订阅链接（未配置为 None）
    pub url: Option<String>,
    /// 上次成功更新时间（RFC 3339 字符串；从未成功为 None）
    pub last_updated: Option<String>,
    /// 订阅节点数（最近一次导入的有效节点数）
    pub node_count: u32,
    /// 手动粘贴导入的节点数
    pub manual_count: u32,
    /// 是否正在更新订阅（前端据此显示加载态）
    pub updating: bool,
    /// 上次更新/导入的失败原因（无失败为 None，前端据此展示降级提示）
    pub last_error: Option<String>,
}

/// 手动粘贴导入的结果（import_share_text 命令返回）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct ImportResult {
    /// 本次成功导入的节点数（已并入手动节点列表）
    pub imported: u32,
    /// 逐条失败明细（坏行不拖垮整体，返回给前端展示）
    pub failures: Vec<NodeFailure>,
}
