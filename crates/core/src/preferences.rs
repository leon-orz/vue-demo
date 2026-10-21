//! 普通偏好的 SQLite 持久化。命名空间由类型化方法固定，调用方不能任意覆盖其他模块。

use std::path::Path;

use serde::{de::DeserializeOwned, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

use crate::models::{AppPreferences, CommandError};

#[derive(Debug, Clone)]
pub struct PreferenceStore {
    pool: SqlitePool,
}

impl PreferenceStore {
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .pragma("journal_mode", "WAL")
            .pragma("busy_timeout", "5000");
        let pool = SqlitePool::connect_with(options).await?;
        crate::db::migrate(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn app(&self) -> Result<Option<AppPreferences>, CommandError> {
        self.read("app").await
    }

    pub async fn set_app(&self, value: &AppPreferences) -> Result<(), CommandError> {
        self.write("app", value).await
    }

    async fn read<T: DeserializeOwned>(&self, namespace: &str) -> Result<Option<T>, CommandError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM preference WHERE namespace = ?")
                .bind(namespace)
                .fetch_optional(&self.pool)
                .await?;
        row.map(|(value,)| {
            serde_json::from_str(&value)
                .map_err(|e| CommandError::internal(format!("{namespace} 偏好解析失败: {e}")))
        })
        .transpose()
    }

    async fn write<T: Serialize>(&self, namespace: &str, value: &T) -> Result<(), CommandError> {
        let json = serde_json::to_string(value)
            .map_err(|e| CommandError::internal(format!("{namespace} 偏好序列化失败: {e}")))?;
        sqlx::query(
            "INSERT INTO preference (namespace, value) VALUES (?, ?)
             ON CONFLICT(namespace) DO UPDATE SET value = excluded.value",
        )
        .bind(namespace)
        .bind(json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ThemeMode;

    #[tokio::test]
    async fn app_preferences_survive_reconnect() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("toogl.db");
        let store = PreferenceStore::connect(&path).await.unwrap();
        let app = AppPreferences {
            theme: ThemeMode::Dark,
        };
        store.set_app(&app).await.unwrap();
        store.pool.close().await;

        let reopened = PreferenceStore::connect(&path).await.unwrap();
        assert_eq!(reopened.app().await.unwrap(), Some(app));
        reopened.pool.close().await;
    }

    #[tokio::test]
    async fn invalid_preferences_return_error_instead_of_overwriting() {
        let tmp = tempfile::tempdir().unwrap();
        let store = PreferenceStore::connect(tmp.path().join("toogl.db"))
            .await
            .unwrap();
        sqlx::query("INSERT INTO preference VALUES ('app', 'invalid json')")
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(store.app().await.is_err());
        store.pool.close().await;
    }
}
