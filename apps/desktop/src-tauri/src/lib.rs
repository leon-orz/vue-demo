mod commands;
// sidecar 状态由 setup 装配，并由代理命令层调用。
pub mod sidecar;

use tauri::Manager as _;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::preferences::app_preferences_get,
            commands::preferences::app_preferences_set,
            commands::proxy::proxy_status,
            commands::proxy::select_proxy,
            commands::proxy::test_delay,
            commands::proxy::set_system_proxy,
            commands::proxy::switch_rule_mode,
            commands::proxy::recover_system_proxy,
            commands::subscription::subscription_status,
            commands::subscription::subscription_set_url,
            commands::subscription::subscription_update,
            commands::subscription::import_share_text,
            commands::system_tools::system_tools_information,
            commands::system_tools::system_tools_scan_cleanup,
            commands::system_tools::system_tools_diagnose_network,
            commands::system_tools::system_tools_flush_dns,
        ])
        .setup(|app| {
            // app data 目录只解析一次，各模块状态共享；解析失败的错误文案也统一
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法解析 app data 目录: {e}"));

            let preferences = match &app_data_dir {
                Ok(dir) => tauri::async_runtime::block_on(
                    commands::preferences::PreferenceState::new(dir.clone()),
                ),
                Err(e) => commands::preferences::PreferenceState::unavailable(e.clone()),
            };
            app.manage(preferences);

            // 内核启动失败转换为不可用状态，保留其余模块和设置的使用路径。
            let proxy_state = match &app_data_dir {
                Ok(dir) => {
                    tauri::async_runtime::block_on(sidecar::start(app.handle(), dir.clone()))
                }
                Err(e) => sidecar::ProxyCoreState::unavailable(e.clone()),
            };
            app.manage(proxy_state);

            // 代理命令状态：持久化设置和系统代理管理器。
            // 系统代理启动自检（上次异常退出残留识别）不在此执行——此处经 block_on
            // 驱动处于 runtime 上下文，直接调 manager 同步方法会嵌套 runtime panic；
            // 自检改为首个 proxy_status / set_system_proxy 调用时经 spawn_blocking
            // 惰性执行一次（见 ProxyCommandState::ensure_startup_check），结果经
            // proxy_status 暴露给前端；恢复由前端确认后调 recover_system_proxy
            let proxy_cmd_state = match &app_data_dir {
                Ok(dir) => tauri::async_runtime::block_on(commands::proxy::ProxyCommandState::new(
                    dir.clone(),
                )),
                Err(e) => commands::proxy::ProxyCommandState::unavailable(e.clone()),
            };
            app.manage(proxy_cmd_state);

            // 订阅状态共享节点目录与更新互斥，不由前端页面持有后台任务。
            let subscription_state = match &app_data_dir {
                Ok(dir) => tauri::async_runtime::block_on(
                    commands::subscription::SubscriptionCommandState::new(dir.clone()),
                ),
                Err(e) => commands::subscription::SubscriptionCommandState::unavailable(e.clone()),
            };
            // 订阅定时更新（24h）：sidecar 就绪与否由任务内部探测（端口/secret 从
            // proxy_setting 自取），未运行只记 last_error 跳过本次，不影响任务存活
            if let Some(updater) = subscription_state.updater() {
                tauri::async_runtime::spawn(commands::subscription::subscription_refresh_loop(
                    updater,
                ));
            }
            app.manage(subscription_state);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            // 本次开启代理产生的备份用于恢复；失败保留备份，供下次启动检查。
            // 恢复失败不能被当作已经恢复成功。
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
