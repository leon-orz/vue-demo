//! MiMo Code 本地日志（mimocode.db）解析与入库（M1 多平台聚合之一，纯本地无网络依赖）。
//!
//! 日志源：`~/.local/share/mimocode/mimocode.db`，SQLite 只读打开（不写 WAL、不加锁）；
//! message 表 data 字段为 JSON，含 tokens / cost / modelID / providerID。
//! 增量以 message 表 rowid 为游标，存进 log_file.offset（path 列为 db 文件路径）。
//! 扫描入口：`MimoStore::scan_and_record(db_path)`；文件缺失优雅降级为空报告。

pub mod parser;
pub mod store;

use std::path::PathBuf;

use thiserror::Error;

use crate::models::CommandError;

pub use store::MimoStore;

/// usage_event / usage_daily / log_file 表中本模块写入的 cli 列取值
/// （platform 列固定为 Platform::Mimo）
pub const MIMO_CLI: &str = "mimocode";

#[derive(Debug, Error)]
pub enum MimoError {
    #[error("日志文件不可读: {0}")]
    Io(#[from] std::io::Error),
    #[error("数据库错误: {0}")]
    Db(#[from] sqlx::Error),
    #[error("无法确定用户主目录（请显式指定日志路径）")]
    NoHomeDir,
}

impl From<MimoError> for CommandError {
    fn from(e: MimoError) -> Self {
        match e {
            // 数据库 / IO / 环境探测失败都归为内部错误
            MimoError::Io(_) | MimoError::Db(_) | MimoError::NoHomeDir => {
                CommandError::internal(e.to_string())
            }
        }
    }
}

/// 默认日志库路径：`~/.local/share/mimocode/mimocode.db`。
pub fn default_db_path() -> Result<PathBuf, MimoError> {
    Ok(dirs::home_dir()
        .ok_or(MimoError::NoHomeDir)?
        .join(".local/share/mimocode/mimocode.db"))
}
