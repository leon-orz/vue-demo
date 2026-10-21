//! 软件与模块偏好各自建模，前端通过 ts-rs 共享契约。敏感凭据不属于偏好。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

/// 影响整个软件的偏好。
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct AppPreferences {
    pub theme: ThemeMode,
}
