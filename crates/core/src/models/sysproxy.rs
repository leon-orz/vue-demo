//! 系统代理（M5）对外模型。读写逻辑见 crate::sysproxy。
//! TS 类型由 ts-rs 生成到 packages/core-types/bindings/，禁止手改生成物。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 系统代理当前状态（读自注册表 HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct SystemProxyStatus {
    /// ProxyEnable 是否开启
    pub enabled: bool,
    /// ProxyServer（host:port），未设置时为 None（关闭状态下也可能残留旧值）
    pub server: Option<String>,
}

/// 启动自检结果：供前端判断是否提示"上次异常退出，系统代理残留开启"。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct SystemProxyCheck {
    /// 当前注册表真实状态
    pub current: SystemProxyStatus,
    /// 是否存在残留：本软件上次开启了系统代理但未正常关闭（崩溃/强杀）
    pub leftover: bool,
    /// 残留时本软件开启前保存的旧值（供提示与无损恢复）；无残留为 None
    pub previous: Option<SystemProxyStatus>,
}
