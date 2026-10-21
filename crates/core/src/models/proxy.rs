//! mihomo 代理（M5）对外模型：代理组/节点摘要、测速结果、规则模式。
//! 读写逻辑见 crate::mihomo::client。
//! TS 类型由 ts-rs 生成到 packages/core-types/bindings/，禁止手改生成物。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::sysproxy::SystemProxyStatus;

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

impl RuleMode {
    /// mihomo /configs 与 proxy_setting 持久化使用的小写字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Global => "global",
            Self::Direct => "direct",
        }
    }

    /// 解析小写模式串（mihomo /configs 响应与 proxy_setting 读取共用）
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rule" => Some(Self::Rule),
            "global" => Some(Self::Global),
            "direct" => Some(Self::Direct),
            _ => None,
        }
    }
}

/// 记住的节点选择（持久化到 proxy_setting，供 UI 初始展示；mihomo 侧才是事实源）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct ProxySelection {
    /// 代理组名
    pub group: String,
    /// 组内选中的节点名
    pub node: String,
}

/// proxy_status 命令返回的聚合状态。各字段独立降级：任一环节失败只让对应字段
/// 为 None/false，不拖垮整体（mihomo 不可达时 proxies/rule_mode 为 None 不算错误）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct ProxyStatus {
    /// sidecar 是否存活（实时探测 mihomo /version）
    pub sidecar_alive: bool,
    /// external-controller 实际端口（9090 顺延后的真实值；sidecar 拉起失败为 None）
    pub controller_port: Option<u16>,
    /// 系统代理开关指向的本地 mixed 端口（mihomo HTTP+SOCKS5 混合入口；
    /// 首跑在 7890~7900 顺延探测，取值优先级：proxy_setting 持久化值 → config.yaml → 7890）
    pub mixed_port: u16,
    /// sidecar 拉起失败原因（正常为 None），供前端展示降级提示
    pub startup_error: Option<String>,
    /// mihomo /proxies 摘要（sidecar 不可用或不可达时为 None）
    pub proxies: Option<ProxiesSummary>,
    /// 当前规则模式（优先读 mihomo /configs；不可达时退化为 proxy_setting 记住的模式）
    pub rule_mode: Option<RuleMode>,
    /// 系统代理当前状态（注册表真实值；读取失败为 None）
    pub system_proxy: Option<SystemProxyStatus>,
    /// 启动自检结果：上次异常退出导致系统代理残留开启（前端据此提示恢复）
    pub system_proxy_leftover: bool,
    /// 残留时本软件开启前保存的旧值（提示文案用；recover 即还原此值）
    pub leftover_previous: Option<SystemProxyStatus>,
    /// 记住的节点选择（供 UI 初始展示兜底；事实源以 proxies[].now 为准）
    pub remembered_selection: Option<ProxySelection>,
    /// mihomo 内核版本与打包期望版本不一致（sidecar 拉起时一次性比对，
    /// 见 sidecar::MIHOMO_EXPECTED_VERSION；仅提示不阻断，sidecar 不可用为 false）
    pub version_mismatch: bool,
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
    #[test]
    fn rule_mode_str_roundtrip() {
        for mode in [RuleMode::Rule, RuleMode::Global, RuleMode::Direct] {
            assert_eq!(RuleMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(RuleMode::parse("RULE"), None);
        assert_eq!(RuleMode::parse(""), None);
    }
}
