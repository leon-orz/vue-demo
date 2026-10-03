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
            commands::proxy::proxy_status,
            commands::proxy::select_proxy,
            commands::proxy::test_delay,
            commands::proxy::set_system_proxy,
            commands::proxy::switch_rule_mode,
            commands::proxy::recover_system_proxy,
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
            let proxy_state = match app.path().app_data_dir() {
                Ok(dir) => tauri::async_runtime::block_on(sidecar::start(app.handle(), dir)),
                Err(e) => {
                    sidecar::ProxyCoreState::unavailable(format!("无法解析 app data 目录: {e}"))
                }
            };
            app.manage(proxy_state);

            // S1-3 项3：proxy 命令层状态（proxy_setting 库 + 系统代理管理器），
            // 构造时跑系统代理启动自检（上次异常退出残留识别），结果经 proxy_status
            // 暴露给前端；恢复由前端确认后调 recover_system_proxy
            let proxy_cmd_state = match app.path().app_data_dir() {
                Ok(dir) => {
                    tauri::async_runtime::block_on(commands::proxy::ProxyCommandState::new(dir))
                }
                Err(e) => commands::proxy::ProxyCommandState::unavailable(format!(
                    "无法解析 app data 目录: {e}"
                )),
            };
            app.manage(proxy_cmd_state);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            // S1-3 项3 退出钩子：本次运行由我们开启的系统代理先无损恢复（有备份即视为
            // 我们开的），保证正常退出不留残留；失败只记录不阻塞，异常退出由启动自检兜底。
            // 先恢复系统代理再杀 mihomo：避免应用流量还指着已退出的本地代理端口。
            if let Some(state) = app_handle.try_state::<commands::proxy::ProxyCommandState>() {
                state.cleanup_system_proxy_on_exit();
            }
            // 事件循环退出前回收 mihomo 子进程，确保不留孤儿进程
            if let Some(state) = app_handle.try_state::<sidecar::ProxyCoreState>() {
                state.shutdown();
            }
        }
    });
}
