mod commands;
mod sidecar;

use tauri::Manager as _;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::keyring::keystore_set,
            commands::keyring::keystore_get,
            commands::keyring::keystore_delete,
            commands::usage::scan_usage_logs,
            commands::usage::usage_summary,
            commands::usage::usage_by_project,
            commands::usage::usage_by_model,
            commands::usage::usage_series,
            commands::usage::quota_refresh,
            commands::usage::quota_latest,
            commands::usage::platform_account_get,
            commands::usage::platform_account_set,
        ])
        .setup(|app| {
            // TODO(S1-3)：拉起 mihomo sidecar（见 sidecar.rs），并做系统代理残留自检
            // 用量模块状态：初始化失败不 panic，降级为错误态（命令统一返回 unavailable），
            // 应用其余模块照常可用
            let state = match app.path().app_data_dir() {
                Ok(dir) => {
                    match tauri::async_runtime::block_on(commands::usage::UsageState::new(dir)) {
                        Ok(state) => state,
                        Err(e) => commands::usage::UsageState::unavailable(e),
                    }
                }
                Err(e) => {
                    commands::usage::UsageState::unavailable(format!("无法解析 app data 目录: {e}"))
                }
            };
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
