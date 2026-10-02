//! Kimi 统计的 SQLite 持久化（扫描入库）。
//! 表结构由 crates/core/migrations/ 定义，经 crate::db 按版本号迁移执行（schema 统一收归 Rust 侧）。
//! 聚合查询已泛化到 usage::query（按 platform 过滤 / 'all' 合计）；
//! 本 store 保留的 summary / usage_by_* / usage_series 只是 Some(Platform::Kimi) 的薄委托。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

use super::scanner::{self, FileCursor, ScanOutput};
use super::{KimiError, KIMI_CLI};
use crate::models::{Platform, ScanReport, UsageGroup, UsageRange, UsageSeries, UsageSummary};

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
    pub async fn migrate(&self) -> Result<(), KimiError> {
        crate::db::migrate(&self.pool).await?;
        Ok(())
    }

    /// 关闭连接池（example / 测试清理临时库文件前调用）
    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn load_cursors(&self) -> Result<HashMap<PathBuf, FileCursor>, KimiError> {
        // 只取本平台的游标（log_file 为多平台共用表，按 platform 隔离）
        let rows: Vec<(String, i64, i64)> =
            sqlx::query_as("SELECT path, offset, mtime FROM log_file WHERE platform = ?")
                .bind(Platform::Kimi.as_str())
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
    pub async fn scan_and_record(&self, sessions_root: &Path) -> Result<ScanReport, KimiError> {
        let _guard = self.scan_lock.lock().await;
        let cursors = self.load_cursors().await?;
        let output = scanner::scan(sessions_root, &cursors)?;
        self.record(output).await
    }

    async fn record(&self, output: ScanOutput) -> Result<ScanReport, KimiError> {
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
            .bind(Platform::Kimi.as_str())
            .bind(path.as_ref())
            .bind(file.new_offset as i64)
            .bind(file.size as i64)
            .bind(file.mtime)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            let (log_file_id,): (i64,) =
                sqlx::query_as("SELECT id FROM log_file WHERE platform = ? AND path = ?")
                    .bind(Platform::Kimi.as_str())
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
            // 个人量级全量重算足够快，页面查询只打聚合表）
            sqlx::query("DELETE FROM usage_daily WHERE platform = ? AND cli = ?")
                .bind(Platform::Kimi.as_str())
                .bind(KIMI_CLI)
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
            .bind(Platform::Kimi.as_str())
            .bind(KIMI_CLI)
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

    // ---- 聚合查询：薄委托 usage::query（泛化查询层，本平台固定 Some(Platform::Kimi)） ----

    pub async fn summary(&self, range: UsageRange) -> Result<UsageSummary, KimiError> {
        Ok(crate::usage::query::summary(&self.pool, Some(Platform::Kimi), range).await?)
    }

    pub async fn usage_by_project(&self, range: UsageRange) -> Result<Vec<UsageGroup>, KimiError> {
        Ok(crate::usage::query::usage_by_project(&self.pool, Some(Platform::Kimi), range).await?)
    }

    pub async fn usage_by_model(&self, range: UsageRange) -> Result<Vec<UsageGroup>, KimiError> {
        Ok(crate::usage::query::usage_by_model(&self.pool, Some(Platform::Kimi), range).await?)
    }

    /// 用量趋势时间序列（Day / Last5Hours 按小时、Week / Month 按日分桶）
    pub async fn usage_series(&self, range: UsageRange) -> Result<UsageSeries, KimiError> {
        Ok(crate::usage::query::usage_series(&self.pool, Some(Platform::Kimi), range).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};
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

        let summary = store.summary(UsageRange::Day).await.unwrap();
        assert_eq!(summary.totals.input_other, 350);
        assert_eq!(summary.totals.output, 35);
        assert_eq!(summary.totals.cache_read, 950);
        // 950 / (350 + 950 + 0)
        assert!((summary.totals.cache_hit_rate.unwrap() - 950.0 / 1300.0).abs() < 1e-9);

        // 再扫一次：无新增（增量游标生效，事件不重复）
        let report2 = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report2.events_added, 0);
        let summary2 = store.summary(UsageRange::Day).await.unwrap();
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
        let summary = store.summary(UsageRange::Day).await.unwrap();
        assert_eq!(summary.totals.input_other, 30);

        // 截断重建 → 旧事件整批替换，不双计
        std::fs::write(&wire, format!("{}\n", usage_line(now, "m1", 5, 5, 5))).unwrap();
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.files_rescanned, 1);
        let summary = store.summary(UsageRange::Day).await.unwrap();
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

        let summary = store.summary(UsageRange::Day).await.unwrap();
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
        sqlx::query("DELETE FROM usage_daily")
            .execute(&store.pool)
            .await
            .unwrap();
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.events_added, 0);
        assert_eq!(report.files_rescanned, 0);
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_daily")
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
        let summary = store.summary(UsageRange::Day).await.unwrap();
        assert_eq!(summary.totals.input_other, 1);

        // 释放占用后重扫：失败文件自然补回
        drop(lock);
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.files_failed, 0);
        let summary = store.summary(UsageRange::Day).await.unwrap();
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
        let last5 = store.summary(UsageRange::Last5Hours).await.unwrap();
        assert_eq!(last5.totals.input_other, 100);

        // 当月范围：月初 1 号 0~6 点跑测试时 six_hours_ago 落在上月，按边界分支断言
        let month = store.summary(UsageRange::Month).await.unwrap();
        let same_month = chrono::Local
            .timestamp_millis_opt(six_hours_ago)
            .single()
            .map(|t| t.format("%Y-%m").to_string())
            == Some(Local::now().format("%Y-%m").to_string());
        let expected = if same_month { 300 } else { 100 };
        assert_eq!(month.totals.input_other, expected);

        let day = store.summary(UsageRange::Day).await.unwrap();
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

        let by_project = store.usage_by_project(UsageRange::Day).await.unwrap();
        assert_eq!(by_project.len(), 2);
        // 按 input_other 降序：blog(200) 在前，app(150) 在后
        assert_eq!(by_project[0].key, "blog");
        assert_eq!(by_project[0].totals.input_other, 200);
        assert_eq!(by_project[1].key, "app");
        assert_eq!(by_project[1].totals.input_other, 150);

        let by_model = store.usage_by_model(UsageRange::Day).await.unwrap();
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
        let summary = store.summary(UsageRange::Day).await.unwrap();
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

        let day = store.usage_series(UsageRange::Day).await.unwrap();
        let same_day = chrono::Local
            .timestamp_millis_opt(now - 2 * 3600 * 1000)
            .single()
            .map(|t| t.format("%Y-%m-%d").to_string())
            == Some(Local::now().format("%Y-%m-%d").to_string());
        // 跨午夜时 2 小时前那条不计入今日
        assert_eq!(day.points.len(), if same_day { 2 } else { 1 });
        assert!(day.points.iter().all(|p| p.bucket.len() == 5));

        let month = store.usage_series(UsageRange::Month).await.unwrap();
        assert!(!month.points.is_empty());
        // 月末桶即今天（本地日期）
        let today = Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(month.points.last().unwrap().bucket, today);
    }
}
