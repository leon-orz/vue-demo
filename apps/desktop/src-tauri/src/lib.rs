mod commands;
// pub：ProxyCoreState 等接口供项3命令层使用，当前仅 setup 装配引用
pub mod sidecar;

use tauri::Manager as _;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
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

            // S1-3：拉起 mihomo sidecar（端口顺延 + 首跑配置生成 + 失败降级，见 sidecar.rs）。
            // 系统代理残留自检（sysproxy startup_check）留到项3命令层做，这里只装配 ProxyCoreState
            let proxy_state = match app.path().app_data_dir() {
                Ok(dir) => tauri::async_runtime::block_on(sidecar::start(app.handle(), dir)),
                Err(e) => {
                    sidecar::ProxyCoreState::unavailable(format!("无法解析 app data 目录: {e}"))
                }
            };
            app.manage(proxy_state);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // 事件循环退出前回收 mihomo 子进程，确保不留孤儿进程
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<sidecar::ProxyCoreState>() {
                state.shutdown();
            }
        }
    });
}
