//! 密钥存取命令：薄壳，实际逻辑在 toogl_core::keystore（keyring-rs 封装）。
//! 前端对应封装见 apps/desktop/src/api/keyring.ts。

use toogl_core::keystore::{KeyringStore, SecretStore};

pub const SERVICE_NAME: &str = "app.toogl.desktop";

fn store() -> KeyringStore {
    KeyringStore::new(SERVICE_NAME)
}

#[tauri::command]
pub fn keystore_set(key: String, value: String) -> Result<(), String> {
    store().set(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn keystore_get(key: String) -> Result<Option<String>, String> {
    store().get(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn keystore_delete(key: String) -> Result<(), String> {
    store().delete(&key).map_err(|e| e.to_string())
}
