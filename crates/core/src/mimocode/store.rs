//! MiMo Code 本地日志（mimocode.db）的增量扫描与入库。
//!
//! 与 kimicode 的文件扫描不同：日志源是单个 SQLite 库，只读打开（`read_only`，
//! 不写 WAL、不加锁），增量以 message 表 rowid 为游标，持久化在 log_file.offset 列
//! （path 列存 db 文件路径）。入库写入多平台共用的 usage_event / usage_daily，
//! 一律带 platform='mimo' 隔离。

use std::path::Path;
use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

use super::parser::{self, ParsedUsage};
use super::{MimoError, MIMO_CLI};
use crate::models::{Platform, ScanReport, UsageEvent};

/// 每批读取的行数：分批消费避免把整张 message 表载入内存
const BATCH_SIZE: u32 = 1000;

#[derive(Debug, Clone)]
pub struct MimoStore {
    pool: SqlitePool,
    /// 扫描互斥锁：并发触发（手动 + 定时任务）时排队执行，避免并发扫描双计入库
    scan_lock: Arc<tokio::sync::Mutex<()>>,
}

/// 一次扫描的产物：解析出的用量、新行游标、是否从头重扫
struct SourceScan {
    parsed: Vec<ParsedUsage>,
    new_offset: i64,
    truncated: bool,
}

impl MimoStore {
    /// 打开（必要时创建）统计数据库，WAL 模式。
    pub async fn connect(db_path: impl AsRef<Path>) -> Result<Self, MimoError> {
        let options = SqliteConnectOptions::new()
            .filename(db_path.as_ref())
            .create_if_missing(true)
            .pragma("journal_mode", "WAL")
            .pragma("busy_timeout", "5000");
        let pool = SqlitePool::connect_with(options).await?;
        Ok(Self::from_pool(pool))
    }

    /// 基于既有连接池构造（usage::UsageService 单池多 store 共享时走这里）
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self {
            pool,
            scan_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// 执行全部未应用的 schema 迁移（幂等，见 crate::db）
    pub async fn migrate(&self) -> Result<(), MimoError> {
        crate::db::migrate(&self.pool).await?;
        Ok(())
    }

    /// 关闭连接池（测试清理临时库文件前调用）
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// 只读打开 mimocode.db（mode=ro：不写 WAL、不加锁），单连接。
    async fn open_source(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .read_only(true);
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
    }

    /// 加载本平台的行游标（log_file 为多平台共用表，按 platform 隔离）
    async fn load_cursor(&self, path: &str) -> Result<i64, sqlx::Error> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT offset FROM log_file WHERE platform = ? AND path = ?")
                .bind(Platform::Mimo.as_str())
                .bind(path)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(o,)| o).unwrap_or(0))
    }

    /// 扫描 mimocode.db 并入库：续读/重扫 → 写事件 → 按需重算日聚合，全程幂等。
    /// 文件缺失时优雅降级为空报告（MiMo Code 未安装是常态，不算故障）；
    /// 文件存在但打不开 / 查不了（非 SQLite、schema 漂移无 message 表等）
    /// 计入 files_failed，游标不动，下次扫描自然重试。
    pub async fn scan_and_record(&self, db_path: &Path) -> Result<ScanReport, MimoError> {
        let _guard = self.scan_lock.lock().await;

        let mut report = ScanReport {
            files_found: 0,
            files_rescanned: 0,
            files_failed: 0,
            events_added: 0,
            partial_lines_skipped: 0,
            bad_lines: 0,
        };

        // 文件缺失：优雅降级，返回空报告（未安装 MiMo Code 不是错误）
        if !db_path.is_file() {
            return Ok(report);
        }
        report.files_found = 1;
        let path = db_path.to_string_lossy().into_owned();

        let meta = match std::fs::metadata(db_path) {
            Ok(m) => m,
            Err(_) => {
                report.files_failed = 1;
                return Ok(report);
            }
        };
        let size = meta.len() as i64;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        // 自家统计库读游标失败属于真实错误（区别于源库故障的降级路径）
        let cursor = self.load_cursor(&path).await?;
        let source = match Self::open_source(db_path).await {
            Ok(p) => p,
            Err(_) => {
                report.files_failed = 1;
                return Ok(report);
            }
        };
        let scan = Self::scan_source(&source, cursor, &mut report).await;
        source.close().await;
        let scan = match scan {
            Ok(s) => s,
            Err(_) => {
                report.files_failed = 1;
                return Ok(report);
            }
        };
        if scan.truncated {
            report.files_rescanned = 1;
        }

        // 归一化为 UsageEvent。
        // project 取舍：message.data 中没有可靠的项目来源（无 cwd / 目录字段），
        // 统一填空串；后续若实测发现可用字段（如 session 表的工作目录）再补推断。
        let events: Vec<UsageEvent> = scan
            .parsed
            .iter()
            .map(|p| UsageEvent {
                platform: Platform::Mimo,
                cli: MIMO_CLI.to_string(),
                ts: p.ts,
                session_id: p.session_id.clone(),
                project: String::new(),
                model: p.model.clone(),
                input_other: p.input_other,
                output: p.output,
                cache_read: p.cache_read,
                cache_creation: p.cache_creation,
                cost: p.cost,
            })
            .collect();
        report.events_added = events.len() as u64;

        self.record(&path, size, mtime, scan.new_offset, &events, scan.truncated)
            .await?;
        Ok(report)
    }

    /// 从源库分批读出待消费行。
    /// 重建检测：游标大于当前最大 rowid，说明库被重建/清空，从头重扫
    /// （record 层据此整批删除旧事件，幂等重建）。
    async fn scan_source(
        source: &SqlitePool,
        cursor: i64,
        report: &mut ScanReport,
    ) -> Result<SourceScan, sqlx::Error> {
        let (max_rowid,): (Option<i64>,) = sqlx::query_as("SELECT MAX(rowid) FROM message")
            .fetch_one(source)
            .await?;
        let truncated = cursor > 0 && max_rowid.map(|m| m < cursor).unwrap_or(true);
        let mut offset = if truncated { 0 } else { cursor };

        let mut parsed = Vec::new();
        loop {
            // 只选 rowid + data 两列：文档只保证 message 表有 data 字段，
            // 少假设列结构以降低 schema 漂移时的失败面（rowid 不存在则整批失败降级）
            let rows: Vec<(i64, String)> = sqlx::query_as(
                "SELECT rowid, data FROM message WHERE rowid > ? ORDER BY rowid LIMIT ?",
            )
            .bind(offset)
            .bind(BATCH_SIZE)
            .fetch_all(source)
            .await?;
            if rows.is_empty() {
                break;
            }
            let batch_len = rows.len();
            for (rowid, data) in rows {
                // 坏行也消费（游标前进到 rowid），避免每次扫描反复重解析
                offset = rowid;
                match parser::parse_data(&data) {
                    // 无 tokens 的行（用户消息等）parse_data 返回 Ok(None)，正常跳过
                    Ok(Some(p)) => parsed.push(p),
                    Ok(None) => {}
                    Err(_) => report.bad_lines += 1,
                }
            }
            if batch_len < BATCH_SIZE as usize {
                break;
            }
        }
        Ok(SourceScan {
            parsed,
            new_offset: offset,
            truncated,
        })
    }

    /// 游标 upsert + 事件写入 + 按需重算日聚合，单事务提交。
    async fn record(
        &self,
        path: &str,
        size: i64,
        mtime: i64,
        new_offset: i64,
        events: &[UsageEvent],
        truncated: bool,
    ) -> Result<(), MimoError> {
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut tx = self.pool.begin().await?;

        // upsert 游标行（platform + path 联合唯一）；offset 语义为已消费的最大 rowid
        sqlx::query(
            "INSERT INTO log_file (platform, path, offset, size, mtime, last_scanned_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(platform, path) DO UPDATE SET
                 offset = excluded.offset, size = excluded.size,
                 mtime = excluded.mtime, last_scanned_at = excluded.last_scanned_at",
        )
        .bind(Platform::Mimo.as_str())
        .bind(path)
        .bind(new_offset)
        .bind(size)
        .bind(mtime)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        let (log_file_id,): (i64,) =
            sqlx::query_as("SELECT id FROM log_file WHERE platform = ? AND path = ?")
                .bind(Platform::Mimo.as_str())
                .bind(path)
                .fetch_one(&mut *tx)
                .await?;
        if truncated {
            // 库被重建：整批删除旧事件后按重扫结果重建
            sqlx::query("DELETE FROM usage_event WHERE log_file_id = ?")
                .bind(log_file_id)
                .execute(&mut *tx)
                .await?;
        }
        for e in events {
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
        }

        // 有新事件或发生重扫时幂等重算本平台日聚合（只删重建本 platform+cli 行组，
        // 不动其他平台数据；个人量级全量重算足够快，页面查询只打聚合表）
        if !events.is_empty() || truncated {
            sqlx::query("DELETE FROM usage_daily WHERE platform = ? AND cli = ?")
                .bind(Platform::Mimo.as_str())
                .bind(MIMO_CLI)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO usage_daily (date, platform, cli, project, model,
                     input_other, output, cache_read, cache_creation)
                 SELECT date(ts / 1000, 'unixepoch', 'localtime'), platform, cli, project, model,
                        SUM(input_other), SUM(output), SUM(cache_read), SUM(cache_creation)
                 FROM usage_event
                 WHERE platform = ? AND cli = ?
                 GROUP BY 1, 2, 3, 4, 5",
            )
            .bind(Platform::Mimo.as_str())
            .bind(MIMO_CLI)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
