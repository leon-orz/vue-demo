//! oh-my-pi（omp）本地日志解析与入库（M1 多平台泛化 S1-4，纯本地无网络依赖）。
//!
//! 实测日志结构（本机 `~/.omp/agent/sessions/` 观察，版本升级可能变更，解析器按行容错）：
//! - 路径：`~/.omp/agent/sessions/{--C--cwd--encoded--}/{开始时间}_{sessionId}.jsonl`
//!   （会话子目录里的 *.bash.log 等附件按扩展名排除）
//! - 会话元信息行：`{"type":"session","id":"..","cwd":"C:\\zone\\2demo","title":".."}`
//!   （实测固定在第 2 行，先于所有消息行；扫描从头重扫时天然先读到）
//! - 用量事件行：`{"type":"message","timestamp":"<RFC3339>","message":{"role":"assistant",`
//!   `"provider":"deepseek","model":"deepseek-v4-flash","stopReason":"stop|toolUse|aborted|..",`
//!   `"timestamp":<Unix毫秒>,"usage":{"input":..,"output":..,"cacheRead":..,"cacheWrite":..,`
//!   `"cost":{"total":..}}}}`；cost 为 omp 按渠道单价计算的美元成本，直接保留进
//!   usage_event.cost（模型注释：日志自带成本字段时保留）
//! - 字段映射：input→input_other、output→output、cacheRead→cache_read、cacheWrite→cache_creation；
//!   model 列存 "provider/model"（如 "deepseek/deepseek-v4-flash"，与 kimi 的 "kimi-code/k1"
//!   同款约定，区分同模型名的不同渠道）
//! - 异常行（stopReason 非 stop/toolUse 或带 errorMessage，如 aborted 中断）：事件带 usage
//!   仍入库（token 已实际消耗），同时计入 ScanReport.bad_lines 诊断指标
//!   （ScanReport 无独立异常行字段，与 JSON 损坏行合并计数，选择及理由见 parser 注释）

pub mod parser;
pub mod scanner;
pub mod store;

use std::path::PathBuf;

use thiserror::Error;

use crate::models::CommandError;

pub use scanner::{FileCursor, ScanOutput, ScanStats};
pub use store::OmpStore;

/// usage_event / usage_daily / log_file 表中本模块写入的 cli 列取值
/// （platform 列固定为 Platform::Omp）
pub const OMP_CLI: &str = "omp";

#[derive(Debug, Error)]
pub enum OmpError {
    #[error("日志目录不可读: {0}")]
    Io(#[from] std::io::Error),
    #[error("数据库错误: {0}")]
    Db(#[from] sqlx::Error),
    #[error("无法确定用户主目录（请显式指定日志目录）")]
    NoHomeDir,
}

impl From<crate::logs::store::LogError> for OmpError {
    fn from(e: crate::logs::store::LogError) -> Self {
        use crate::logs::store::LogError;
        match e {
            LogError::Io(e) => OmpError::Io(e),
            LogError::Db(e) => OmpError::Db(e),
        }
    }
}

impl From<OmpError> for CommandError {
    fn from(e: OmpError) -> Self {
        match e {
            // 数据库 / IO / 环境探测失败都归为内部错误
            OmpError::Io(_) | OmpError::Db(_) | OmpError::NoHomeDir => {
                CommandError::internal(e.to_string())
            }
        }
    }
}

/// 会话日志根目录（~/.omp/agent/sessions）。
pub fn default_sessions_dir() -> Result<PathBuf, OmpError> {
    Ok(dirs::home_dir()
        .ok_or(OmpError::NoHomeDir)?
        .join(".omp")
        .join("agent")
        .join("sessions"))
}
