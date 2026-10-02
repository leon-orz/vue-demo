//! omp 用量的 SQLite 持久化：扫描结果入库（游标 upsert / 事件追加 / 截断重建 / 日聚合重算）。
//! 写表方式与 kimicode::store 一致（多平台共用表按 platform 隔离，本平台 platform="omp"、
//! cli="omp"）；聚合查询由 S1-4 的 DirectSource 跨平台查询层承担，本 store 只负责写入。
//! 表结构由 crates/core/migrations/ 定义，经 crate::db 按版本号迁移执行（schema 统一收归 Rust 侧）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{SecondsFormat, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

use super::scanner::{self, FileCursor, ScanOutput};
use super::{OmpError, OMP_CLI};
use crate::models::{Platform, ScanReport};

#[derive(Debug, Clone)]
pub struct OmpStore {
    pool: SqlitePool,
    /// 扫描互斥锁：并发触发（手动 + 定时任务）时排队执行，避免并发扫描双计入库
    scan_lock: Arc<tokio::sync::Mutex<()>>,
}

impl OmpStore {
    /// 打开（必要时创建）数据库，WAL 模式。
    pub async fn connect(db_path: impl AsRef<Path>) -> Result<Self, OmpError> {
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
    pub async fn migrate(&self) -> Result<(), OmpError> {
        crate::db::migrate(&self.pool).await?;
        Ok(())
    }

    /// 关闭连接池（测试清理临时库文件前调用）
    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn load_cursors(&self) -> Result<HashMap<PathBuf, FileCursor>, OmpError> {
        // 只取本平台的游标（log_file 为多平台共用表，按 platform 隔离）
        let rows: Vec<(String, i64, i64)> =
            sqlx::query_as("SELECT path, offset, mtime FROM log_file WHERE platform = ?")
                .bind(Platform::Omp.as_str())
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
    pub async fn scan_and_record(&self, sessions_root: &Path) -> Result<ScanReport, OmpError> {
        let _guard = self.scan_lock.lock().await;
        let cursors = self.load_cursors().await?;
        let output = scanner::scan(sessions_root, &cursors)?;
        self.record(output).await
    }

    async fn record(&self, output: ScanOutput) -> Result<ScanReport, OmpError> {
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
            .bind(Platform::Omp.as_str())
            .bind(path.as_ref())
            .bind(file.new_offset as i64)
            .bind(file.size as i64)
            .bind(file.mtime)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            let (log_file_id,): (i64,) =
                sqlx::query_as("SELECT id FROM log_file WHERE platform = ? AND path = ?")
                    .bind(Platform::Omp.as_str())
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
                .bind(Platform::Omp.as_str())
                .bind(OMP_CLI)
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
            .bind(Platform::Omp.as_str())
            .bind(OMP_CLI)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    async fn temp_store() -> (tempfile::TempDir, OmpStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = OmpStore::connect(tmp.path().join("test.db")).await.unwrap();
        store.migrate().await.unwrap();
        (tmp, store)
    }

    fn make_session(root: &Path, dir: &str, session: &str, cwd: &str, lines: &[String]) -> PathBuf {
        let session_dir = root.join(dir);
        std::fs::create_dir_all(&session_dir).unwrap();
        let file = session_dir.join(format!("2026-09-21T06-56-06-434Z_{session}.jsonl"));
        let mut f = std::fs::File::create(&file).unwrap();
        writeln!(f, r#"{{"type":"title","title":"t"}}"#).unwrap();
        writeln!(
            f,
            r#"{{"type":"session","version":3,"id":"{session}","timestamp":"2026-09-21T06:56:06.434Z","cwd":{cwd:?}}}"#
        )
        .unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        file
    }

    fn assistant_line(ts: i64, model: &str, input: i64, output: i64, cache_read: i64) -> String {
        format!(
            r#"{{"type":"message","timestamp":"2026-09-21T07:18:03.566Z","message":{{"role":"assistant","provider":"deepseek","model":"{model}","usage":{{"input":{input},"output":{output},"cacheRead":{cache_read},"cacheWrite":0,"cost":{{"total":0.001}}}},"stopReason":"stop","timestamp":{ts}}}}}"#
        )
    }

    async fn event_totals(store: &OmpStore) -> (i64, i64, i64) {
        sqlx::query_as(
            "SELECT COALESCE(SUM(input_other),0), COALESCE(SUM(output),0), COALESCE(SUM(cache_read),0)
             FROM usage_event WHERE platform = 'omp'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn scan_persist_roundtrip() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        make_session(
            tmp.path(),
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[
                assistant_line(now - 1000, "deepseek-v4-flash", 100, 10, 900),
                assistant_line(now - 2000, "deepseek-v4-pro", 50, 5, 50),
            ],
        );
        make_session(
            tmp.path(),
            "--C--work-blog--",
            "s2",
            "C:/work/blog",
            &[assistant_line(now - 3000, "deepseek-v4-flash", 200, 20, 0)],
        );

        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.files_found, 2);
        assert_eq!(report.events_added, 3);
        assert_eq!(event_totals(&store).await, (350, 35, 950));

        // 事件字段核对：platform/cli/model/project/cost
        let row: (String, String, String, String, Option<f64>) = sqlx::query_as(
            "SELECT platform, cli, model, project, cost FROM usage_event
             WHERE platform = 'omp' ORDER BY ts DESC LIMIT 1",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(row.0, "omp");
        assert_eq!(row.1, "omp");
        assert_eq!(row.2, "deepseek/deepseek-v4-flash");
        assert_eq!(row.3, "app");
        assert_eq!(row.4, Some(0.001));

        // 日聚合已按 platform+cli 重建
        let (daily_input,): (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(input_other),0) FROM usage_daily WHERE platform = 'omp'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(daily_input, 350);

        // 再扫一次：无新增（增量游标生效，事件不重复）
        let report2 = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report2.events_added, 0);
        assert_eq!(event_totals(&store).await, (350, 35, 950));
    }

    #[tokio::test]
    async fn append_and_truncation_trigger_idempotent_rescan() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        let file = make_session(
            tmp.path(),
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[assistant_line(now, "m1", 10, 1, 0)],
        );
        store.scan_and_record(tmp.path()).await.unwrap();

        // 追加一行 → size 变化触发从头重扫：旧事件整批替换为全量（2 条），不双计
        let mut f = std::fs::File::options().append(true).open(&file).unwrap();
        writeln!(f, "{}", assistant_line(now, "m1", 20, 2, 0)).unwrap();
        drop(f);
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.events_added, 2);
        assert_eq!(report.files_rescanned, 1);
        assert_eq!(event_totals(&store).await, (30, 3, 0));

        // 截断重建 → 旧事件整批替换，不双计
        std::fs::write(&file, format!("{}\n", assistant_line(now, "m1", 5, 5, 5))).unwrap();
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.files_rescanned, 1);
        assert_eq!(event_totals(&store).await, (5, 5, 5));
    }

    #[tokio::test]
    async fn concurrent_scans_are_serialized_without_double_counting() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        make_session(
            tmp.path(),
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[
                assistant_line(now, "m1", 10, 1, 0),
                assistant_line(now, "m1", 20, 2, 0),
            ],
        );

        let (r1, r2) = tokio::join!(
            store.scan_and_record(tmp.path()),
            store.scan_and_record(tmp.path())
        );
        let (r1, r2) = (r1.unwrap(), r2.unwrap());
        assert_eq!(r1.events_added + r2.events_added, 2);
        assert_eq!(event_totals(&store).await, (30, 3, 0));
    }

    #[tokio::test]
    async fn rescan_does_not_touch_other_platforms() {
        let (tmp, store) = temp_store().await;
        let now = Utc::now().timestamp_millis();
        // 预置其他平台数据（模拟 kimi 已入库）
        sqlx::query(
            "INSERT INTO log_file (platform, path, offset, size, mtime, last_scanned_at)
             VALUES ('kimi', '/other/wire.jsonl', 1, 1, 1, '2026-01-01T00:00:00Z')",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO usage_daily (date, platform, cli, project, model,
                 input_other, output, cache_read, cache_creation)
             VALUES ('2026-01-01', 'kimi', 'kimi-code', 'app', 'm1', 999, 9, 9, 9)",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        make_session(
            tmp.path(),
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[assistant_line(now, "m1", 10, 1, 0)],
        );
        store.scan_and_record(tmp.path()).await.unwrap();

        // kimi 的日聚合行不受影响
        let (kimi_input,): (i64,) =
            sqlx::query_as("SELECT input_other FROM usage_daily WHERE platform = 'kimi'")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(kimi_input, 999);
        // kimi 的游标不受影响
        let (cnt,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM log_file WHERE platform = 'kimi'")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(cnt, 1);
    }
}
