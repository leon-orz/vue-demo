//! 节点导入（M5-07）：订阅拉取 + 分享链接/订阅内容解析 + mihomo 配置接线与托管节点文件。
//!
//! 分层：
//! - [`share`]：单条分享链接（vless / hysteria2(hy2) / trojan / ss）→ mihomo proxies 条目，
//!   纯函数便于单测；
//! - [`content`]：订阅/粘贴内容形态识别（Clash/mihomo YAML 的 proxies 段、base64 编码的
//!   分享链接列表、纯文本分享链接列表），逐条容错（坏行记 failures，不拖垮整体）；
//! - [`fetch`]：订阅链接拉取（reqwest，超时 + UA 伪装，错误归 Fetch）；
//! - [`config`]：托管节点文件（toogl-nodes.yaml）读写与 config.yaml 的一次性接线
//!   （proxy-providers file provider + 「节点选择」组 use），用户手写配置不动。
//!
//! 编排（订阅元信息持久化、定时更新、热重载）在桌面端 src-tauri 侧（subscription.rs）。

pub mod config;
pub mod content;
pub mod fetch;
pub mod share;

pub use config::{ensure_wired, restore_nodes_file, write_nodes_file, NODES_FILE_NAME};
pub use content::{parse_nodes_text, NodeFailure, ParsedNodes};
pub use fetch::{fetch_subscription, validate_subscription_url};
pub use share::parse_share_link;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SubscriptionError {
    /// 订阅拉取失败（超时 / 连接失败 / 非 2xx）
    #[error("订阅拉取失败: {0}")]
    Fetch(String),
    /// 内容整体无法识别（不是 YAML proxies、也不是分享链接列表）
    #[error("订阅内容解析失败: {0}")]
    Parse(String),
    /// 单条分享链接非法（逐条解析时记入 failures，仅在链接本身被单独解析时返回）
    #[error("分享链接非法: {0}")]
    Link(String),
    /// 配置 / 托管节点文件读写失败
    #[error("配置写入失败: {0}")]
    Io(String),
    /// config.yaml 缺 Toogl 接线且非本工具模板（用户手写配置），需手工补接线
    #[error("配置未接线: {0}")]
    Wiring(String),
}
