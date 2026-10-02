//! Tauri 命令的结构化错误类型：所有命令统一返回 `Result<T, CommandError>`，
//! 前端 invoke 抛出的就是本类型的 JSON（契约，字段签名不得擅自变更）。
//! TS 类型由 ts-rs 生成到 packages/core-types/bindings/，禁止手改生成物。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 结构化命令错误。`kind` 供前端程序化分支，`message` 为可直接展示的中文文案。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct CommandError {
    /// 错误类别，取值约定：
    /// unauthorized（401/403 认证失败）、network（超时/连接失败）、
    /// not_found（未配置/无记录）、internal（内部错误）、unavailable（模块降级不可用）
    pub kind: String,
    /// 面向用户的可读错误文案
    pub message: String,
}

impl CommandError {
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.to_string(),
            message: message.into(),
        }
    }

    /// 认证失败（HTTP 401/403）
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new("unauthorized", message)
    }

    /// 网络错误（超时 / 连接失败）
    pub fn network(message: impl Into<String>) -> Self {
        Self::new("network", message)
    }

    /// 未配置 / 无记录
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }

    /// 内部错误（数据库、IO、解析等）
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }

    /// 模块降级不可用（如启动初始化失败、账户停用）
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new("unavailable", message)
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message)
    }
}

impl std::error::Error for CommandError {}

/// 数据库错误统一归为 internal（编排层用 ? 直接转换）
impl From<sqlx::Error> for CommandError {
    fn from(e: sqlx::Error) -> Self {
        CommandError::internal(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        CommandError::export().unwrap();
    }

    #[test]
    fn serializes_to_contract_json() {
        let err = CommandError::unauthorized("请检查 API key");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "unauthorized", "message": "请检查 API key"})
        );
    }
}
