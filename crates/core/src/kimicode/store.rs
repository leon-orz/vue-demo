//! Kimi 统计的 SQLite 持久化与聚合查询。
//! 表结构由 crates/core/migrations/ 定义，经 crate::db 按版本号迁移执行（schema 统一收归 Rust 侧）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Datelike, Local, SecondsFormat, TimeZone, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

use super::pricing;
use super::scanner::{self, FileCursor, ScanOutput};
use super::KimiError;
use crate::models::{
    KimiBalance, KimiScanReport, KimiUsageGroup, KimiUsagePoint, KimiUsageRange, KimiUsageSeries,
    KimiUsageSummary, KimiUsageTotals,
};

/// 账户表行（内部类型；对前端暴露的是 models::KimiAccountInfo，has_credential 由命令层补充）
#[derive(Debug, Clone)]
pub struct KimiAccountRow {
    pub name: String,
    pub api_base: String,
    pub credential_ref: String,
    pub enabled: bool,
}

/// group_by 的分组列白名单（枚举约束，禁止外部输入拼接 SQL）
#[derive(Debug, Clone, Copy)]
enum GroupBy {
    Project,
    Model,
}

impl GroupBy {
    fn column(self) -> &'static str {
        match self {
            GroupBy::Project => "project",
            GroupBy::Model => "model",
        }
    }
}

#[derive(Debug, Clone)]
pub struct KimiStore {
    pool: SqlitePool,
    /// 扫描互斥锁：并发触发（手动 + 定时任务）时排队执行，避免并发扫描双计入库
    scan_lock: Arc<tokio::sync::Mutex<()>>,
}

impl KimiStore {
    /// 打开（必要时创建）数据库，WAL 模式。
    pub async fn connect(db_path: impl AsRef<Path>) -> Result<Self, KimiError> {
        let options = SqliteConnectOptions::new()
            .filename(db_path.as_ref())
            .create_if_missing(true)
            .pragma("journal_mode", "WAL")
            .pragma("busy_timeout", "5000");
        let pool = SqlitePool::connect_with(options).await?;
        Ok(Self {
            pool,
            scan_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// 执行全部未应用的 schema 迁移（幂等，见 crate::db）
    pub async fn migrate(&self) -> Result<(), KimiError> {
        crate::db::migrate(&self.pool).await?;
        Ok(())
    }

    /// 关闭连接池（example / 测试清理临时库文件前调用）
    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn load_cursors(&self) -> Result<HashMap<PathBuf, FileCursor>, KimiError> {
        let rows: Vec<(String, i64, i64)> =
            sqlx::query_as("SELECT path, offset, mtime FROM kimi_log_file")
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

    /// 扫描并入库：重扫/续读 → 写事件 → 按需重算日聚合，全程幂等。
    /// 并发调用经互斥锁排队，保证不会双计入库。
    pub async fn scan_and_record(&self, sessions_root: &Path) -> Result<KimiScanReport, KimiError> {
        let _guard = self.scan_lock.lock().await;
        let cursors = self.load_cursors().await?;
        let output = scanner::scan(sessions_root, &cursors)?;
        self.record(output).await
    }

    async fn record(&self, output: ScanOutput) -> Result<KimiScanReport, KimiError> {
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut events_added: u64 = 0;
        let has_rescan = output.files.iter().any(|f| f.truncated);
        let mut tx = self.pool.begin().await?;
        for file in &output.files {
            let path = file.path.to_string_lossy();
            // upsert 游标行
            sqlx::query(
                "INSERT INTO kimi_log_file (path, offset, size, mtime, last_scanned_at)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(path) DO UPDATE SET
                     offset = excluded.offset, size = excluded.size,
                     mtime = excluded.mtime, last_scanned_at = excluded.last_scanned_at",
            )
            .bind(path.as_ref())
            .bind(file.new_offset as i64)
            .bind(file.size as i64)
            .bind(file.mtime)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            let (log_file_id,): (i64,) =
                sqlx::query_as("SELECT id FROM kimi_log_file WHERE path = ?")
                    .bind(path.as_ref())
                    .fetch_one(&mut *tx)
                    .await?;
            if file.truncated {
                // 文件被截断/重建/改写：整批删除旧事件后按重扫结果重建
                sqlx::query("DELETE FROM kimi_usage_event WHERE log_file_id = ?")
                    .bind(log_file_id)
                    .execute(&mut *tx)
                    .await?;
            }
            for e in &file.events {
                sqlx::query(
                    "INSERT INTO kimi_usage_event
                         (log_file_id, ts, session_id, project, model,
                          input_other, output, cache_read, cache_creation)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(log_file_id)
                .bind(e.ts)
                .bind(&e.session_id)
                .bind(&e.project)
                .bind(&e.model)
                .bind(e.input_other)
                .bind(e.output)
                .bind(e.cache_read)
                .bind(e.cache_creation)
                .execute(&mut *tx)
                .await?;
                events_added += 1;
            }
        }
        // 本次扫描无新事件且无重扫时跳过日聚合全量重建（结果必然相同）
        if events_added > 0 || has_rescan {
            // 幂等重算日聚合表（个人量级全量重算足够快；页面查询只打聚合表）
            sqlx::query("DELETE FROM kimi_usage_daily")
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT INTO kimi_usage_daily (date, project, model,
                     input_other, output, cache_read, cache_creation)
                 SELECT date(ts / 1000, 'unixepoch', 'localtime'), project, model,
                        SUM(input_other), SUM(output), SUM(cache_read), SUM(cache_creation)
                 FROM kimi_usage_event
                 GROUP BY 1, 2, 3",
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;

        Ok(KimiScanReport {
            files_found: output.stats.files_found,
            files_rescanned: output.stats.files_rescanned,
            files_failed: output.stats.files_failed,
            events_added,
            partial_lines_skipped: output.stats.partial_lines_skipped,
            bad_lines: output.stats.bad_lines,
        })
    }

    /// 各范围的 SQL 来源：天/周/月打预聚合表（按起始日过滤），最近 5 小时走事件表
    fn range_source(range: KimiUsageRange) -> (String, i64) {
        match range {
            KimiUsageRange::Last5Hours => {
                let cutoff = Utc::now().timestamp_millis() - 5 * 3600 * 1000;
                ("event".to_string(), cutoff)
            }
            KimiUsageRange::Day => ("daily".to_string(), 0),
            KimiUsageRange::Week => ("daily".to_string(), 1),
            KimiUsageRange::Month => ("daily".to_string(), 2),
        }
    }

    fn range_start_date(kind: i64) -> String {
        let today = Local::now().date_naive();
        let date = match kind {
            0 => today,
            // 本周一起
            1 => today.week(chrono::Weekday::Mon).first_day(),
            // 本月 1 号起
            _ => today.with_day(1).unwrap_or(today),
        };
        date.format("%Y-%m-%d").to_string()
    }

    /// 填充估值金额（单价表在业务层 kimicode::pricing，数据模型不反向依赖）
    fn with_estimated_cost(mut totals: KimiUsageTotals) -> KimiUsageTotals {
        totals.estimated_cost = pricing::estimate_cost(
            totals.input_other,
            totals.output,
            totals.cache_read,
            totals.cache_creation,
        );
        totals
    }

    async fn totals_where(&self, range: KimiUsageRange) -> Result<KimiUsageTotals, KimiError> {
        let (source, marker) = Self::range_source(range);
        let row: (i64, i64, i64, i64) = if source == "event" {
            sqlx::query_as(
                "SELECT COALESCE(SUM(input_other),0), COALESCE(SUM(output),0),
                        COALESCE(SUM(cache_read),0), COALESCE(SUM(cache_creation),0)
                 FROM kimi_usage_event WHERE ts >= ?",
            )
            .bind(marker)
            .fetch_one(&self.pool)
            .await?
        } else {
            let start = Self::range_start_date(marker);
            sqlx::query_as(
                "SELECT COALESCE(SUM(input_other),0), COALESCE(SUM(output),0),
                        COALESCE(SUM(cache_read),0), COALESCE(SUM(cache_creation),0)
                 FROM kimi_usage_daily WHERE date >= ?",
            )
            .bind(start)
            .fetch_one(&self.pool)
            .await?
        };
        Ok(Self::with_estimated_cost(KimiUsageTotals::new(
            row.0, row.1, row.2, row.3,
        )))
    }

    pub async fn summary(&self, range: KimiUsageRange) -> Result<KimiUsageSummary, KimiError> {
        Ok(KimiUsageSummary {
            range,
            totals: self.totals_where(range).await?,
        })
    }

    /// 按项目 / 按模型分组
    async fn group_by(
        &self,
        range: KimiUsageRange,
        group: GroupBy,
    ) -> Result<Vec<KimiUsageGroup>, KimiError> {
        let group_col = group.column();
        let (source, marker) = Self::range_source(range);
        let rows: Vec<(String, i64, i64, i64, i64)> = if source == "event" {
            sqlx::query_as(&format!(
                "SELECT {group_col}, SUM(input_other), SUM(output),
                        SUM(cache_read), SUM(cache_creation)
                 FROM kimi_usage_event WHERE ts >= ?
                 GROUP BY {group_col} ORDER BY 2 DESC"
            ))
            .bind(marker)
            .fetch_all(&self.pool)
            .await?
        } else {
            let start = Self::range_start_date(marker);
            sqlx::query_as(&format!(
                "SELECT {group_col}, SUM(input_other), SUM(output),
                        SUM(cache_read), SUM(cache_creation)
                 FROM kimi_usage_daily WHERE date >= ?
                 GROUP BY {group_col} ORDER BY 2 DESC"
            ))
            .bind(start)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(key, a, b, c, d)| KimiUsageGroup {
                key,
                totals: Self::with_estimated_cost(KimiUsageTotals::new(a, b, c, d)),
            })
            .collect())
    }

    pub async fn usage_by_project(
        &self,
        range: KimiUsageRange,
    ) -> Result<Vec<KimiUsageGroup>, KimiError> {
        self.group_by(range, GroupBy::Project).await
    }

    pub async fn usage_by_model(
        &self,
        range: KimiUsageRange,
    ) -> Result<Vec<KimiUsageGroup>, KimiError> {
        self.group_by(range, GroupBy::Model).await
    }

    /// 用量趋势时间序列（M1 趋势图）：
    /// Day / Last5Hours 按小时分桶（事件表，本地时区），Week / Month 按日分桶（预聚合表）
    pub async fn usage_series(&self, range: KimiUsageRange) -> Result<KimiUsageSeries, KimiError> {
        let rows: Vec<(String, i64, i64, i64, i64)> = match range {
            KimiUsageRange::Day | KimiUsageRange::Last5Hours => {
                let marker = match range {
                    KimiUsageRange::Day => Self::local_midnight_ms(),
                    KimiUsageRange::Last5Hours => Utc::now().timestamp_millis() - 5 * 3600 * 1000,
                    _ => unreachable!(),
                };
                sqlx::query_as(
                    "SELECT strftime('%H:00', ts / 1000, 'unixepoch', 'localtime') AS bucket,
                            SUM(input_other), SUM(output), SUM(cache_read), SUM(cache_creation)
                     FROM kimi_usage_event WHERE ts >= ?
                     GROUP BY bucket ORDER BY bucket",
                )
                .bind(marker)
                .fetch_all(&self.pool)
                .await?
            }
            KimiUsageRange::Week | KimiUsageRange::Month => {
                let (_, kind) = Self::range_source(range);
                let start = Self::range_start_date(kind);
                sqlx::query_as(
                    "SELECT date AS bucket,
                            SUM(input_other), SUM(output), SUM(cache_read), SUM(cache_creation)
                     FROM kimi_usage_daily WHERE date >= ?
                     GROUP BY date ORDER BY date",
                )
                .bind(start)
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(KimiUsageSeries {
            range,
            points: rows
                .into_iter()
                .map(|(bucket, a, b, c, d)| KimiUsagePoint {
                    bucket,
                    totals: Self::with_estimated_cost(KimiUsageTotals::new(a, b, c, d)),
                })
                .collect(),
        })
    }

    /// 今日零点（本地时区）的 Unix 毫秒时间戳；极端情况下回退为当前时刻（宁可少算，不错算昨日）
    fn local_midnight_ms() -> i64 {
        Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|dt| Local.from_local_datetime(&dt).single())
            .map(|dt| dt.timestamp_millis())
            .unwrap_or_else(|| Utc::now().timestamp_millis())
    }

    pub async fn account_get(&self) -> Result<Option<KimiAccountRow>, KimiError> {
        let row: Option<(String, String, String, i64)> = sqlx::query_as(
            "SELECT name, api_base, credential_ref, enabled FROM kimi_account WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(
            row.map(|(name, api_base, credential_ref, enabled)| KimiAccountRow {
                name,
                api_base,
                credential_ref,
                enabled: enabled != 0,
            }),
        )
    }

    pub async fn account_set(
        &self,
        name: &str,
        api_base: &str,
        credential_ref: &str,
        enabled: bool,
    ) -> Result<(), KimiError> {
        sqlx::query(
            "INSERT INTO kimi_account (id, name, api_base, credential_ref, enabled)
             VALUES (1, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name, api_base = excluded.api_base,
                 credential_ref = excluded.credential_ref, enabled = excluded.enabled",
        )
        .bind(name)
        .bind(api_base)
        .bind(credential_ref)
        .bind(enabled as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn save_balance(&self, balance: &KimiBalance) -> Result<(), KimiError> {
        sqlx::query(
            "INSERT INTO kimi_balance_snapshot (available, voucher, cash, fetched_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(balance.available)
        .bind(balance.voucher)
        .bind(balance.cash)
        .bind(&balance.fetched_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn latest_balance(&self) -> Result<Option<KimiBalance>, KimiError> {
        let row: Option<(f64, f64, f64, String)> = sqlx::query_as(
            "SELECT available, voucher, cash, fetched_at
             FROM kimi_balance_snapshot ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(
            row.map(|(available, voucher, cash, fetched_at)| KimiBalance {
                available,
                voucher,
                cash,
                fetched_at,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::io::Write;

    async fn temp_store() -> (tempfile::TempDir, KimiStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = KimiStore::connect(tmp.path().join("test.db"))
            .await
            .unwrap();
        store.migrate().await.unwrap();
        (tmp, store)
    }

    fn usage_line(ts: i64, model: &str, input: i64, output: i64, cache_read: i64) -> String {
        format!(
            r#"{{"type":"usage.record","model":"{model}","usage":{{"inputOther":{input},"output":{output},"inputCacheRead":{cache_read},"inputCacheCreation":0}},"usageScope":"turn","time":{ts}}}"#
        )
    }

    fn make_session(root: &Path, wd: &str, session: &str, cwd: &str, lines: &[String]) {
        let session_dir = root.join(wd).join(session);
        let agents = session_dir.join("agents").join("main");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            session_dir.join("state.json"),
            format!(r#"{{"id":"{session}","cwd":{cwd:?}}}"#),
        )
        .unwrap();
        let mut f = std::fs::File::create(agents.join("wire.jsonl")).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[tokio::test]
    async fn scan_persist_and_summary_roundtrip() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        make_session(
            tmp.path(),
            "wd_app_a",
            "session_s1",
            "C:/work/app",
            &[
                usage_line(now - 1000, "m1", 100, 10, 900),
                usage_line(now - 2000, "m2", 50, 5, 50),
            ],
        );
        make_session(
            tmp.path(),
            "wd_blog_b",
            "session_s2",
            "C:/work/blog",
            &[usage_line(now - 3000, "m1", 200, 20, 0)],
        );

        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.files_found, 2);
        assert_eq!(report.events_added, 3);

        let summary = store.summary(KimiUsageRange::Day).await.unwrap();
        assert_eq!(summary.totals.input_other, 350);
        assert_eq!(summary.totals.output, 35);
        assert_eq!(summary.totals.cache_read, 950);
        // 950 / (350 + 950 + 0)
        assert!((summary.totals.cache_hit_rate.unwrap() - 950.0 / 1300.0).abs() < 1e-9);

        // 再扫一次：无新增（增量游标生效，事件不重复）
        let report2 = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report2.events_added, 0);
        let summary2 = store.summary(KimiUsageRange::Day).await.unwrap();
        assert_eq!(summary2.totals.input_other, 350);
    }

    #[tokio::test]
    async fn append_and_truncation_trigger_idempotent_rescan() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        make_session(
            tmp.path(),
            "wd_app_a",
            "session_s1",
            "C:/work/app",
            &[usage_line(now, "m1", 10, 1, 0)],
        );
        store.scan_and_record(tmp.path()).await.unwrap();

        // 追加一行 → size 变化触发从头重扫：旧事件整批替换为全量（2 条），不双计
        let wire = tmp
            .path()
            .join("wd_app_a/session_s1/agents/main/wire.jsonl");
        let mut f = std::fs::File::options().append(true).open(&wire).unwrap();
        writeln!(f, "{}", usage_line(now, "m1", 20, 2, 0)).unwrap();
        drop(f);
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.events_added, 2);
        assert_eq!(report.files_rescanned, 1);
        let summary = store.summary(KimiUsageRange::Day).await.unwrap();
        assert_eq!(summary.totals.input_other, 30);

        // 截断重建 → 旧事件整批替换，不双计
        std::fs::write(&wire, format!("{}\n", usage_line(now, "m1", 5, 5, 5))).unwrap();
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.files_rescanned, 1);
        let summary = store.summary(KimiUsageRange::Day).await.unwrap();
        assert_eq!(summary.totals.input_other, 5);
        assert_eq!(summary.totals.output, 5);
    }

    #[tokio::test]
    async fn concurrent_scans_are_serialized_without_double_counting() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        make_session(
            tmp.path(),
            "wd_app_a",
            "session_s1",
            "C:/work/app",
            &[
                usage_line(now, "m1", 10, 1, 0),
                usage_line(now, "m1", 20, 2, 0),
                usage_line(now, "m2", 30, 3, 0),
            ],
        );

        // 并发触发两次扫描：互斥锁排队执行，第二次读到游标后无增量
        let (r1, r2) = tokio::join!(
            store.scan_and_record(tmp.path()),
            store.scan_and_record(tmp.path())
        );
        let (r1, r2) = (r1.unwrap(), r2.unwrap());
        assert_eq!(r1.events_added + r2.events_added, 3);

        let summary = store.summary(KimiUsageRange::Day).await.unwrap();
        assert_eq!(summary.totals.input_other, 60);
        assert_eq!(summary.totals.output, 6);
    }

    #[tokio::test]
    async fn skips_daily_rebuild_when_no_new_events() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        make_session(
            tmp.path(),
            "wd_app_a",
            "session_s1",
            "C:/work/app",
            &[usage_line(now, "m1", 10, 1, 0)],
        );
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.events_added, 1);

        // 破坏日聚合表后做无增量扫描：不应触发重算（表保持被破坏状态）
        sqlx::query("DELETE FROM kimi_usage_daily")
            .execute(&store.pool)
            .await
            .unwrap();
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.events_added, 0);
        assert_eq!(report.files_rescanned, 0);
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM kimi_usage_daily")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    /// 单文件失败容错（store 层贯通）：不可读文件计入 files_failed、游标不更新，恢复后重扫入库。
    #[cfg(windows)]
    #[tokio::test]
    async fn failed_file_skipped_and_retried_next_scan() {
        use std::os::windows::fs::OpenOptionsExt;
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        make_session(
            tmp.path(),
            "wd_good",
            "session_g",
            "C:/work/good",
            &[usage_line(now, "m1", 1, 1, 0)],
        );
        make_session(
            tmp.path(),
            "wd_bad",
            "session_b",
            "C:/work/bad",
            &[usage_line(now, "m1", 2, 2, 0)],
        );
        let bad_wire = tmp.path().join("wd_bad/session_b/agents/main/wire.jsonl");
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&bad_wire)
            .unwrap();

        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.files_found, 2);
        assert_eq!(report.files_failed, 1);
        assert_eq!(report.events_added, 1);
        let summary = store.summary(KimiUsageRange::Day).await.unwrap();
        assert_eq!(summary.totals.input_other, 1);

        // 释放占用后重扫：失败文件自然补回
        drop(lock);
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.files_failed, 0);
        let summary = store.summary(KimiUsageRange::Day).await.unwrap();
        assert_eq!(summary.totals.input_other, 3);
    }

    #[tokio::test]
    async fn aggregation_range_boundaries() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now();
        let now_ms = now.timestamp_millis();
        // 今天 1 小时前（在最近 5 小时窗口内）
        let recent = now_ms - 3600 * 1000;
        // 6 小时前（落在最近 5 小时窗口外；跨日/跨月边界时计入哪个范围需按实际情况断言）
        let six_hours_ago = now_ms - 6 * 3600 * 1000;
        make_session(
            tmp.path(),
            "wd_app_a",
            "session_s1",
            "C:/work/app",
            &[
                usage_line(recent, "m1", 100, 10, 0),
                usage_line(six_hours_ago, "m1", 200, 20, 0),
            ],
        );
        store.scan_and_record(tmp.path()).await.unwrap();

        // 最近 5 小时只含 recent
        let last5 = store.summary(KimiUsageRange::Last5Hours).await.unwrap();
        assert_eq!(last5.totals.input_other, 100);

        // 当月范围：月初 1 号 0~6 点跑测试时 six_hours_ago 落在上月，按边界分支断言
        let month = store.summary(KimiUsageRange::Month).await.unwrap();
        let same_month = chrono::Local
            .timestamp_millis_opt(six_hours_ago)
            .single()
            .map(|t| t.format("%Y-%m").to_string())
            == Some(Local::now().format("%Y-%m").to_string());
        let expected = if same_month { 300 } else { 100 };
        assert_eq!(month.totals.input_other, expected);

        let day = store.summary(KimiUsageRange::Day).await.unwrap();
        let same_day = chrono::Local
            .timestamp_millis_opt(six_hours_ago)
            .single()
            .map(|t| t.format("%Y-%m-%d").to_string())
            == Some(Local::now().format("%Y-%m-%d").to_string());
        let expected = if same_day { 300 } else { 100 };
        assert_eq!(day.totals.input_other, expected);
    }

    #[tokio::test]
    async fn group_by_project_and_model() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        make_session(
            tmp.path(),
            "wd_app_a",
            "session_s1",
            "C:/work/app",
            &[
                usage_line(now, "m1", 100, 10, 100),
                usage_line(now, "m2", 50, 30, 0),
            ],
        );
        make_session(
            tmp.path(),
            "wd_blog_b",
            "session_s2",
            "C:/work/blog",
            &[usage_line(now, "m1", 200, 20, 0)],
        );
        store.scan_and_record(tmp.path()).await.unwrap();

        let by_project = store.usage_by_project(KimiUsageRange::Day).await.unwrap();
        assert_eq!(by_project.len(), 2);
        // 按 input_other 降序：blog(200) 在前，app(150) 在后
        assert_eq!(by_project[0].key, "blog");
        assert_eq!(by_project[0].totals.input_other, 200);
        assert_eq!(by_project[1].key, "app");
        assert_eq!(by_project[1].totals.input_other, 150);

        let by_model = store.usage_by_model(KimiUsageRange::Day).await.unwrap();
        assert_eq!(by_model.len(), 2);
        assert_eq!(by_model[0].key, "m1");
        assert_eq!(by_model[0].totals.input_other, 300);
        // m1 命中率 = 100 / (300 + 100)
        assert_eq!(by_model[0].totals.cache_hit_rate, Some(0.25));
        assert_eq!(by_model[1].key, "m2");
        assert_eq!(by_model[1].totals.cache_hit_rate, Some(0.0));
    }

    #[tokio::test]
    async fn cache_hit_rate_null_when_no_input() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        make_session(
            tmp.path(),
            "wd_app_a",
            "session_s1",
            "C:/work/app",
            &[usage_line(now, "m1", 0, 42, 0)],
        );
        store.scan_and_record(tmp.path()).await.unwrap();
        let summary = store.summary(KimiUsageRange::Day).await.unwrap();
        assert_eq!(summary.totals.cache_hit_rate, None);
        assert_eq!(summary.totals.output, 42);
    }

    #[tokio::test]
    async fn usage_series_buckets() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        // 1 小时前与 2 小时前各一条：Day 序列应出现两个相邻小时桶
        make_session(
            tmp.path(),
            "wd_app_a",
            "session_s1",
            "C:/work/app",
            &[
                usage_line(now - 3600 * 1000, "m1", 100, 10, 0),
                usage_line(now - 2 * 3600 * 1000, "m1", 200, 20, 0),
            ],
        );
        store.scan_and_record(tmp.path()).await.unwrap();

        let day = store.usage_series(KimiUsageRange::Day).await.unwrap();
        let same_day = chrono::Local
            .timestamp_millis_opt(now - 2 * 3600 * 1000)
            .single()
            .map(|t| t.format("%Y-%m-%d").to_string())
            == Some(Local::now().format("%Y-%m-%d").to_string());
        // 跨午夜时 2 小时前那条不计入今日
        assert_eq!(day.points.len(), if same_day { 2 } else { 1 });
        assert!(day.points.iter().all(|p| p.bucket.len() == 5));

        let month = store.usage_series(KimiUsageRange::Month).await.unwrap();
        assert!(!month.points.is_empty());
        // 月末桶即今天（本地日期）
        let today = Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(month.points.last().unwrap().bucket, today);
    }

    #[tokio::test]
    async fn account_roundtrip_and_balance_snapshot() {
        let (tmp, store) = temp_store().await;
        let _ = tmp;
        assert!(store.account_get().await.unwrap().is_none());

        store
            .account_set("主账号", "https://api.moonshot.cn", "kimi-key", true)
            .await
            .unwrap();
        let row = store.account_get().await.unwrap().unwrap();
        assert_eq!(row.name, "主账号");
        assert_eq!(row.credential_ref, "kimi-key");
        assert!(row.enabled);

        store
            .account_set("主账号", "https://api.moonshot.cn", "kimi-key", false)
            .await
            .unwrap();
        let row = store.account_get().await.unwrap().unwrap();
        assert!(!row.enabled);

        assert!(store.latest_balance().await.unwrap().is_none());
        let balance = KimiBalance {
            available: 10.5,
            voucher: 2.0,
            cash: 8.5,
            fetched_at: "2026-01-01T00:00:00Z".to_string(),
        };
        store.save_balance(&balance).await.unwrap();
        assert_eq!(store.latest_balance().await.unwrap(), Some(balance));
    }
}
