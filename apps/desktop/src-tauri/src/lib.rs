mod commands;
mod sidecar;

use tauri::Manager as _;
use tauri_plugin_sql::{Migration, MigrationKind};

/// SQLite migration 列表。新增 migration 时：
/// 1. 在 migrations/ 下新增 NNNN_description.sql
/// 2. 在此处追加一条 Migration（version 严格递增，只增不改）
fn migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            description: "create_platform_and_usage_snapshot",
            sql: include_str!("../migrations/0001_init.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "create_kimi_tables",
            sql: include_str!("../migrations/0002_kimi.sql"),
            kind: MigrationKind::Up,
        },
    ]
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:toogl.db", migrations())
                .build(),
        )
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
            // Kimi 统计状态：SQLite 连接池 + 默认日志目录（与 tauri-plugin-sql 共用 toogl.db）
            let app_data_dir = app.path().app_data_dir().expect("无法解析 app data 目录");
            let state =
                tauri::async_runtime::block_on(commands::kimi::KimiState::new(app_data_dir))
                    .expect("Kimi 存储初始化失败");
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
