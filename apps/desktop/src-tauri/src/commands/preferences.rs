//! 软件偏好命令薄壳，持久化与规范化由 core 负责。

use std::path::PathBuf;

use tauri::State;
use toogl_core::models::{AppPreferences, CommandError};
use toogl_core::preferences::PreferenceStore;

pub struct PreferenceState(Result<PreferenceStore, String>);

impl PreferenceState {
    pub async fn new(dir: PathBuf) -> Self {
        Self(
            PreferenceStore::connect(dir.join("toogl.db"))
                .await
                .map_err(|e| e.to_string()),
        )
    }

    pub fn unavailable(reason: String) -> Self {
        Self(Err(reason))
    }

    fn require(&self) -> Result<&PreferenceStore, CommandError> {
        self.0
            .as_ref()
            .map_err(|e| CommandError::unavailable(format!("偏好存储初始化失败: {e}")))
    }
}

#[tauri::command]
pub async fn app_preferences_get(
    state: State<'_, PreferenceState>,
) -> Result<Option<AppPreferences>, CommandError> {
    state.require()?.app().await
}

#[tauri::command]
pub async fn app_preferences_set(
    state: State<'_, PreferenceState>,
    preferences: AppPreferences,
) -> Result<(), CommandError> {
    state.require()?.set_app(&preferences).await
}
