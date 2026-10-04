//! Tauri 命令的结构化错误类型：所有命令统一返回 `Result<T, CommandError>`，
//! 前端 invoke 抛出的就是本类型的 JSON（契约，字段签名不得擅自变更）。
//! TS 类型由 ts-rs 生成到 packages/core-types/bindings/，禁止手改生成物。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 命令错误类别（serde snake_case，与历史字符串契约一致）。前端白名单校验
/// 直接对照 ts-rs 生成的联合类型，不再手写。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub enum CommandErrorKind {
    /// 认证失败（HTTP 401/403）
    Unauthorized,
    /// 网络错误（超时 / 连接失败）
    Network,
    /// 未配置 / 无记录
    NotFound,
    /// 内部错误（数据库、IO、解析等）
    Internal,
    /// 模块降级不可用（如启动初始化失败、账户停用）
    Unavailable,
}

impl CommandErrorKind {
    /// snake_case 字符串（Display 与调试输出用）
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::Network => "network",
            Self::NotFound => "not_found",
            Self::Internal => "internal",
            Self::Unavailable => "unavailable",
        }
    }
}

impl std::fmt::Display for CommandErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 结构化命令错误。`kind` 供前端程序化分支，`message` 为可直接展示的中文文案。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct CommandError {
    /// 错误类别（见 [`CommandErrorKind`]）
    pub kind: CommandErrorKind,
    /// 面向用户的可读错误文案
    pub message: String,
}

impl CommandError {
    pub fn new(kind: CommandErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 认证失败（HTTP 401/403）
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(CommandErrorKind::Unauthorized, message)
    }

    /// 网络错误（超时 / 连接失败）
    pub fn network(message: impl Into<String>) -> Self {
        Self::new(CommandErrorKind::Network, message)
    }

    /// 未配置 / 无记录
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(CommandErrorKind::NotFound, message)
    }

    /// 内部错误（数据库、IO、解析等）
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(CommandErrorKind::Internal, message)
    }

    /// 模块降级不可用（如启动初始化失败、账户停用）
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(CommandErrorKind::Unavailable, message)
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
        CommandErrorKind::export().unwrap();
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

    #[test]
    fn kind_serializes_snake_case() {
        for (kind, s) in [
            (CommandErrorKind::Unauthorized, "unauthorized"),
            (CommandErrorKind::Network, "network"),
            (CommandErrorKind::NotFound, "not_found"),
            (CommandErrorKind::Internal, "internal"),
            (CommandErrorKind::Unavailable, "unavailable"),
        ] {
            assert_eq!(serde_json::to_value(kind).unwrap(), serde_json::json!(s));
            assert_eq!(kind.as_str(), s);
            assert_eq!(kind.to_string(), s);
            // 反序列化往返
            assert_eq!(
                serde_json::from_value::<CommandErrorKind>(serde_json::json!(s)).unwrap(),
                kind
            );
        }
    }
}
