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
            commands::kimi::scan_kimi_logs,
            commands::kimi::kimi_usage_summary,
            commands::kimi::kimi_usage_by_project,
            commands::kimi::kimi_usage_by_model,
            commands::kimi::kimi_usage_series,
            commands::kimi::kimi_balance_refresh,
            commands::kimi::kimi_balance_latest,
            commands::kimi::kimi_account_get,
            commands::kimi::kimi_account_set,
        ])
        .setup(|app| {
            // TODO(S1-3)：拉起 mihomo sidecar（见 sidecar.rs），并做系统代理残留自检
            // Kimi 模块状态：初始化失败不 panic，降级为错误态（命令统一返回 unavailable），
            // 应用其余模块照常可用
            let state = match app.path().app_data_dir() {
                Ok(dir) => {
                    match tauri::async_runtime::block_on(commands::kimi::KimiState::new(dir)) {
                        Ok(state) => state,
                        Err(e) => commands::kimi::KimiState::unavailable(e),
                    }
                }
                Err(e) => {
                    commands::kimi::KimiState::unavailable(format!("无法解析 app data 目录: {e}"))
                }
            };
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
