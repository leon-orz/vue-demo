use tauri::async_runtime;
use toogl_core::models::{
    CleanupScan, CommandError, NetworkDiagnostic, NetworkRepairResult, SystemInformation,
};

#[tauri::command]
pub async fn system_tools_information() -> Result<SystemInformation, CommandError> {
    async_runtime::spawn_blocking(toogl_core::system_tools::system_information)
        .await
        .map_err(|e| CommandError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn system_tools_scan_cleanup() -> Result<CleanupScan, CommandError> {
    async_runtime::spawn_blocking(toogl_core::system_tools::scan_temp_directory)
        .await
        .map_err(|e| CommandError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn system_tools_diagnose_network() -> Result<NetworkDiagnostic, CommandError> {
    Ok(toogl_core::system_tools::diagnose_network().await)
}

#[tauri::command]
pub async fn system_tools_flush_dns() -> Result<NetworkRepairResult, CommandError> {
    async_runtime::spawn_blocking(toogl_core::system_tools::flush_dns)
        .await
        .map_err(|e| CommandError::internal(e.to_string()))?
}
