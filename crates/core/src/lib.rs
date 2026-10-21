//! Toogl 共享核心 crate。
//! 桌面端 Tauri commands 是本 crate 的薄壳，各工具业务按模块组织。

pub mod db;
pub mod mihomo;
pub mod models;
pub mod preferences;
pub mod proxy_setting;
pub mod subscription;
pub mod sysproxy;
#[cfg(windows)]
pub mod system_tools;
