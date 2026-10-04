//! 文件游标日志的公共入库骨架：游标 upsert / 事件追加 / 截断整批重建 / 日聚合重算 /
//! 扫描互斥锁。当前只有 kimicode 的 store 是它的薄壳（仅差 platform / cli 常量与扫描函数）；
//! 骨架按平台泛化保留，后续回归的平台 parser 直接复用。
//! 写表方式：多平台共用表按 platform 隔离（log_file 按 platform+path 联合唯一，
//! usage_daily 只删重建本 platform+cli 行组，不动其他平台数据）。
//! 表结构由 crates/core/migrations/ 定义，经 crate::db 按版本号迁移执行（schema 统一收归 Rust 侧）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use thiserror::Error;

use super::scanner::{FileCursor, ScanOutput};
use crate::models::{Platform, ScanReport};

/// 公共骨架的错误类型；平台 store 薄壳经 From 转换为本模块错误（KimiError）
#[derive(Debug, Error)]
pub enum LogError {
    #[error("日志目录不可读: {0}")]
    Io(#[from] std::io::Error),
    #[error("数据库错误: {0}")]
    Db(#[from] sqlx::Error),
}

/// 文件游标日志的通用 store：扫描并入库（重扫 → 写事件 → 按需重算日聚合），全程幂等。
#[derive(Debug, Clone)]
pub struct LogStore {
    pool: SqlitePool,
    platform: Platform,
    cli: &'static str,
    /// 扫描互斥锁：并发触发（手动 + 定时任务）时排队执行，避免并发扫描双计入库
    scan_lock: Arc<tokio::sync::Mutex<()>>,
}

impl LogStore {
    /// 打开（必要时创建）数据库，WAL 模式。
    pub async fn connect(
        db_path: impl AsRef<Path>,
        platform: Platform,
        cli: &'static str,
    ) -> Result<Self, LogError> {
        let options = SqliteConnectOptions::new()
            .filename(db_path.as_ref())
            .create_if_missing(true)
            .pragma("journal_mode", "WAL")
            .pragma("busy_timeout", "5000");
        let pool = SqlitePool::connect_with(options).await?;
        Ok(Self::from_pool(pool, platform, cli))
    }

    /// 基于既有连接池构造（usage::UsageService 单池多 store 共享时走这里）
    pub fn from_pool(pool: SqlitePool, platform: Platform, cli: &'static str) -> Self {
        Self {
            pool,
            platform,
            cli,
            scan_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// 执行全部未应用的 schema 迁移（幂等，见 crate::db）
    pub async fn migrate(&self) -> Result<(), LogError> {
        crate::db::migrate(&self.pool).await?;
        Ok(())
    }

    /// 关闭连接池（example / 测试清理临时库文件前调用）
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// 连接池访问：薄壳 store 的聚合查询委托（usage::query）与测试断言用
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    async fn load_cursors(&self) -> Result<HashMap<PathBuf, FileCursor>, LogError> {
        // 只取本平台的游标（log_file 为多平台共用表，按 platform 隔离）
        let rows: Vec<(String, i64, i64)> =
            sqlx::query_as("SELECT path, offset, mtime FROM log_file WHERE platform = ?")
                .bind(self.platform.as_str())
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(p, o, m)| {
                (
                    PathBuf::from(p),
                    FileCursor {
                        offset: o as u64,
                        mtime: m,
                    },
                )
            })
            .collect())
    }

    /// 扫描并入库：重扫 → 写事件 → 按需重算日聚合，全程幂等。
    /// `scan` 为平台的扫描函数（crate::logs::scanner::scan 装好收集规则与行解析回调）。
    /// 并发调用经互斥锁排队，保证不会双计入库。
    pub async fn scan_and_record(
        &self,
        sessions_root: &Path,
        scan: fn(&Path, &HashMap<PathBuf, FileCursor>) -> std::io::Result<ScanOutput>,
    ) -> Result<ScanReport, LogError> {
        let _guard = self.scan_lock.lock().await;
        let cursors = self.load_cursors().await?;
        let output = scan(sessions_root, &cursors)?;
        self.record(output).await
    }

    async fn record(&self, output: ScanOutput) -> Result<ScanReport, LogError> {
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut events_added: u64 = 0;
        let has_rescan = output.files.iter().any(|f| f.truncated);
        let mut tx = self.pool.begin().await?;
        for file in &output.files {
            let path = file.path.to_string_lossy();
            // upsert 游标行（platform + path 联合唯一）
            sqlx::query(
                "INSERT INTO log_file (platform, path, offset, size, mtime, last_scanned_at)
                 VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT(platform, path) DO UPDATE SET
                     offset = excluded.offset, size = excluded.size,
                     mtime = excluded.mtime, last_scanned_at = excluded.last_scanned_at",
            )
            .bind(self.platform.as_str())
            .bind(path.as_ref())
            .bind(file.new_offset as i64)
            .bind(file.size as i64)
            .bind(file.mtime)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            let (log_file_id,): (i64,) =
                sqlx::query_as("SELECT id FROM log_file WHERE platform = ? AND path = ?")
                    .bind(self.platform.as_str())
                    .bind(path.as_ref())
                    .fetch_one(&mut *tx)
                    .await?;
            if file.truncated {
                // 文件被截断/重建/改写：整批删除旧事件后按重扫结果重建
                sqlx::query("DELETE FROM usage_event WHERE log_file_id = ?")
                    .bind(log_file_id)
                    .execute(&mut *tx)
                    .await?;
            }
            for e in &file.events {
                sqlx::query(
                    "INSERT INTO usage_event
                         (platform, cli, log_file_id, ts, session_id, project, model,
                          input_other, output, cache_read, cache_creation, cost)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(e.platform.as_str())
                .bind(&e.cli)
                .bind(log_file_id)
                .bind(e.ts)
                .bind(&e.session_id)
                .bind(&e.project)
                .bind(&e.model)
                .bind(e.input_other)
                .bind(e.output)
                .bind(e.cache_read)
                .bind(e.cache_creation)
                .bind(e.cost)
                .execute(&mut *tx)
                .await?;
                events_added += 1;
            }
        }
        // 本次扫描无新事件且无重扫时跳过日聚合全量重建（结果必然相同）
        if events_added > 0 || has_rescan {
            // 幂等重算本平台日聚合（只删重建 platform+cli 本行组，不动其他平台数据；
            // 个人量级全量重算足够快，页面查询只打聚合表；request_count 为该桶事件行数）
            sqlx::query("DELETE FROM usage_daily WHERE platform = ? AND cli = ?")
                .bind(self.platform.as_str())
                .bind(self.cli)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO usage_daily (date, platform, cli, project, model,
                     input_other, output, cache_read, cache_creation, request_count)
                 SELECT date(ts / 1000, 'unixepoch', 'localtime'), platform, cli, project, model,
                        SUM(input_other), SUM(output), SUM(cache_read), SUM(cache_creation),
                        COUNT(*)
                 FROM usage_event
                 WHERE platform = ? AND cli = ?
                 GROUP BY 1, 2, 3, 4, 5",
            )
            .bind(self.platform.as_str())
            .bind(self.cli)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;

        Ok(ScanReport {
            files_found: output.stats.files_found,
            files_rescanned: output.stats.files_rescanned,
            files_failed: output.stats.files_failed,
            events_added,
            partial_lines_skipped: output.stats.partial_lines_skipped,
            bad_lines: output.stats.bad_lines,
        })
    }
}
