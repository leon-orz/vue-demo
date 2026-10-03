//! mihomo 代理（M5）对外模型：代理组/节点摘要、测速结果、规则模式。
//! 读写逻辑见 crate::mihomo::client。
//! TS 类型由 ts-rs 生成到 packages/core-types/bindings/，禁止手改生成物。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 节点延迟状态（mihomo 惯例：history 末条 delay=0 视为超时，空 history 为未测）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub enum DelayStatus {
    /// 无测速记录
    Untested,
    /// 测速超时
    Timeout,
    /// 已测得延迟（见 delay_ms）
    Delayed,
}

/// 代理节点摘要
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct ProxyNode {
    /// 节点名
    pub name: String,
    /// 节点协议类型（vless / hysteria2 / ...），取自 mihomo 的 type 字段
    pub node_type: String,
    /// 最近延迟（毫秒）；超时/未测为 None
    pub delay_ms: Option<u32>,
    pub delay_status: DelayStatus,
}

/// 代理组类型（mihomo type 字段；未识别的归 Unknown，不拖垮解析）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub enum ProxyGroupType {
    Selector,
    UrlTest,
    Fallback,
    LoadBalance,
    Relay,
    Unknown,
}

impl ProxyGroupType {
    /// 解析 mihomo 的 type 字符串（Selector / URLTest / Fallback / LoadBalance / Relay）
    pub fn parse(s: &str) -> Self {
        match s {
            "Selector" => Self::Selector,
            "URLTest" => Self::UrlTest,
            "Fallback" => Self::Fallback,
            "LoadBalance" => Self::LoadBalance,
            "Relay" => Self::Relay,
            _ => Self::Unknown,
        }
    }
}

/// 代理组摘要（含组内节点）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct ProxyGroup {
    /// 组名
    pub name: String,
    pub group_type: ProxyGroupType,
    /// 当前选中的节点名
    pub now: String,
    /// 组内节点
    pub nodes: Vec<ProxyNode>,
}

/// GET /proxies 的聚合摘要（组按名字排序，保证前端渲染稳定）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct ProxiesSummary {
    pub groups: Vec<ProxyGroup>,
}

/// 单节点测速结果；批量测速中单节点失败只记入自身 error，不中断整批
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct NodeDelayResult {
    /// 节点名
    pub name: String,
    /// 延迟（毫秒）；超时/失败为 None
    pub delay_ms: Option<u32>,
    pub delay_status: DelayStatus,
    /// 请求级错误（不可达 / Api / 解析失败）；成功为 None
    pub error: Option<String>,
}

/// 规则模式（PATCH /configs 的 mode 取值，序列化为小写）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub enum RuleMode {
    Rule,
    Global,
    Direct,
}

/// mihomo 内核版本（GET /version，兼作存活探测）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct MihomoVersion {
    pub version: String,
    /// 是否 mihomo（Clash.Meta）内核；字段缺失为 None
    pub meta: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 运行本测试即重新生成 packages/core-types/bindings（CI 校验生成物与仓库一致）
    #[test]
    fn export_bindings() {
        DelayStatus::export().unwrap();
        ProxyNode::export().unwrap();
        ProxyGroupType::export().unwrap();
        ProxyGroup::export().unwrap();
        ProxiesSummary::export().unwrap();
        NodeDelayResult::export().unwrap();
        RuleMode::export().unwrap();
        MihomoVersion::export().unwrap();
    }
}
