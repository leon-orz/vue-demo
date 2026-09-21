mod commands;
mod sidecar;

use tauri_plugin_sql::{Migration, MigrationKind};

/// SQLite migration 列表。新增 migration 时：
/// 1. 在 migrations/ 下新增 NNNN_description.sql
/// 2. 在此处追加一条 Migration（version 严格递增，只增不改）
fn migrations() -> Vec<Migration> {
    vec![Migration {
        version: 1,
        description: "create_platform_and_usage_snapshot",
        sql: include_str!("../migrations/0001_init.sql"),
        kind: MigrationKind::Up,
    }]
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
        ])
        .setup(|_app| {
            // TODO(S1-3)：拉起 mihomo sidecar（见 sidecar.rs），并做系统代理残留自检
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
