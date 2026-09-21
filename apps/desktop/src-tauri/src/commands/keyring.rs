//! 密钥存取命令：薄壳，实际逻辑在 toogl_core::keystore（keyring-rs 封装）。
//! 前端对应封装见 apps/desktop/src/api/keyring.ts。

use toogl_core::keystore::{KeyringStore, SecretStore, KEYRING_SERVICE_NAME};
use toogl_core::models::CommandError;

fn store() -> KeyringStore {
    KeyringStore::new(KEYRING_SERVICE_NAME)
}

#[tauri::command]
pub fn keystore_set(key: String, value: String) -> Result<(), CommandError> {
    store().set(&key, &value).map_err(CommandError::from)
}

#[tauri::command]
pub fn keystore_get(key: String) -> Result<Option<String>, CommandError> {
    store().get(&key).map_err(CommandError::from)
}

#[tauri::command]
pub fn keystore_delete(key: String) -> Result<(), CommandError> {
    store().delete(&key).map_err(CommandError::from)
}
