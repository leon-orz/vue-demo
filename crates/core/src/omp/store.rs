//! omp 用量的 SQLite 持久化：扫描入库走 crate::logs::store::LogStore 公共骨架
//! （游标 upsert / 事件追加 / 截断整批重建 / 日聚合重算 / 扫描互斥锁），
//! 本 store 只是 platform="omp"、cli="omp" 的薄壳；聚合查询由 usage::query 跨平台查询层承担。
//! 表结构由 crates/core/migrations/ 定义，经 crate::db 按版本号迁移执行（schema 统一收归 Rust 侧）。

use std::path::Path;

use sqlx::sqlite::SqlitePool;

use super::scanner;
use super::{OmpError, OMP_CLI};
use crate::logs::store::LogStore;
use crate::models::{Platform, ScanReport};

#[derive(Debug, Clone)]
pub struct OmpStore {
    inner: LogStore,
}

impl OmpStore {
    /// 打开（必要时创建）数据库，WAL 模式。
    pub async fn connect(db_path: impl AsRef<Path>) -> Result<Self, OmpError> {
        Ok(Self {
            inner: LogStore::connect(db_path, Platform::Omp, OMP_CLI).await?,
        })
    }

    /// 基于既有连接池构造（usage::UsageService 单池多 store 共享时走这里）
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self {
            inner: LogStore::from_pool(pool, Platform::Omp, OMP_CLI),
        }
    }

    /// 执行全部未应用的 schema 迁移（幂等，见 crate::db）
    pub async fn migrate(&self) -> Result<(), OmpError> {
        Ok(self.inner.migrate().await?)
    }

    /// 关闭连接池（测试清理临时库文件前调用）
    pub async fn close(&self) {
        self.inner.close().await;
    }

    /// 扫描并入库：重扫 → 写事件 → 按需重算日聚合，全程幂等。
    /// 并发调用经互斥锁排队，保证不会双计入库。
    pub async fn scan_and_record(&self, sessions_root: &Path) -> Result<ScanReport, OmpError> {
        Ok(self
            .inner
            .scan_and_record(sessions_root, scanner::scan)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::io::Write;
    use std::path::PathBuf;

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
        .fetch_one(store.inner.pool())
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
        .fetch_one(store.inner.pool())
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
        .fetch_one(store.inner.pool())
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
        .execute(store.inner.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO usage_daily (date, platform, cli, project, model,
                 input_other, output, cache_read, cache_creation)
             VALUES ('2026-01-01', 'kimi', 'kimi-code', 'app', 'm1', 999, 9, 9, 9)",
        )
        .execute(store.inner.pool())
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
                .fetch_one(store.inner.pool())
                .await
                .unwrap();
        assert_eq!(kimi_input, 999);
        // kimi 的游标不受影响
        let (cnt,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM log_file WHERE platform = 'kimi'")
                .fetch_one(store.inner.pool())
                .await
                .unwrap();
        assert_eq!(cnt, 1);
    }
}
