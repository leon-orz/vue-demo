//! Windows 系统工具页面使用的只读快照与操作结果。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct SystemInformation {
    pub windows_edition: String,
    pub version: String,
    pub architecture: String,
    pub computer_name: String,
    pub processor: String,
    pub total_memory_bytes: u64,
    pub system_drive_free_bytes: u64,
    pub system_drive_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct CleanupScan {
    pub files: u64,
    pub bytes: u64,
    pub skipped_entries: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct NetworkDiagnostic {
    pub dns_resolution: bool,
    pub tcp_connection: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct NetworkRepairResult {
    pub success: bool,
    pub message: String,
}
