//! Kimi Code 本地日志（wire.jsonl）解析与聚合（M1 核心，纯本地无网络依赖）。
//!
//! 实测日志结构（版本升级可能变更，解析器按行容错）：
//! - 路径：`{KIMI_CODE_HOME 或 ~/.kimi-code}/sessions/{wd_*}/{session_*}/agents/*/wire.jsonl`
//! - 用量事件：`{"type":"usage.record","model":..,"time":<Unix毫秒>,"usageScope":"turn",`
//!   `"usage":{"inputOther":..,"output":..,"inputCacheRead":..,"inputCacheCreation":..}}`
//! - `usageScope:"session"` 是会话末次上下文快照（非新增消耗），聚合时只计 turn 级事件
//! - 同会话目录 `state.json` 的 `cwd` 字段提供项目路径（取末级目录名展示）

pub mod parser;
pub mod pricing;
pub mod scanner;
pub mod store;

use std::path::PathBuf;

use thiserror::Error;

pub use scanner::{ScanOutput, ScanStats};
pub use store::KimiStore;

#[derive(Debug, Error)]
pub enum KimiError {
    #[error("日志目录不可读: {0}")]
    Io(#[from] std::io::Error),
    #[error("数据库错误: {0}")]
    Db(#[from] sqlx::Error),
    #[error("无法确定用户主目录（请显式指定日志目录）")]
    NoHomeDir,
}

/// 会话日志根目录（sessions 目录）。
/// `KIMI_CODE_HOME` 环境变量优先，其次默认 `~/.kimi-code`。
pub fn default_sessions_dir() -> Result<PathBuf, KimiError> {
    let home = match std::env::var_os("KIMI_CODE_HOME") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => dirs::home_dir()
            .ok_or(KimiError::NoHomeDir)?
            .join(".kimi-code"),
    };
    Ok(home.join("sessions"))
}
