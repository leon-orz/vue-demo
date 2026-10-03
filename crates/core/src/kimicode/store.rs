//! Kimi 统计的 SQLite 持久化（扫描入库）。
//! 扫描入库骨架（游标 upsert / 截断整批重建 / 日聚合重算 / 扫描互斥锁）收在
//! crate::logs::store::LogStore，本 store 只是 platform="kimi"、cli="kimi-code" 的薄壳。
//! 表结构由 crates/core/migrations/ 定义，经 crate::db 按版本号迁移执行（schema 统一收归 Rust 侧）。
//! 聚合查询已泛化到 usage::query（按 platform 过滤 / 'all' 合计）；
//! 本 store 保留的 summary / usage_by_* / usage_series 只是 Some(Platform::Kimi) 的薄委托。

use std::path::Path;

use sqlx::sqlite::SqlitePool;

use super::scanner;
use super::{KimiError, KIMI_CLI};
use crate::logs::store::LogStore;
use crate::models::{Platform, ScanReport, UsageGroup, UsageRange, UsageSeries, UsageSummary};

#[derive(Debug, Clone)]
pub struct KimiStore {
    inner: LogStore,
}

impl KimiStore {
    /// 打开（必要时创建）数据库，WAL 模式。
    pub async fn connect(db_path: impl AsRef<Path>) -> Result<Self, KimiError> {
        Ok(Self {
            inner: LogStore::connect(db_path, Platform::Kimi, KIMI_CLI).await?,
        })
    }

    /// 基于既有连接池构造（usage::UsageService 单池多 store 共享时走这里）
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self {
            inner: LogStore::from_pool(pool, Platform::Kimi, KIMI_CLI),
        }
    }

    /// 执行全部未应用的 schema 迁移（幂等，见 crate::db）
    pub async fn migrate(&self) -> Result<(), KimiError> {
        Ok(self.inner.migrate().await?)
    }

    /// 关闭连接池（example / 测试清理临时库文件前调用）
    pub async fn close(&self) {
        self.inner.close().await;
    }

    /// 扫描并入库：重扫 → 写事件 → 按需重算日聚合，全程幂等。
    /// 并发调用经互斥锁排队，保证不会双计入库。
    pub async fn scan_and_record(&self, sessions_root: &Path) -> Result<ScanReport, KimiError> {
        Ok(self
            .inner
            .scan_and_record(sessions_root, scanner::scan)
            .await?)
    }

    // ---- 聚合查询：薄委托 usage::query（泛化查询层，本平台固定 Some(Platform::Kimi)） ----

    pub async fn summary(&self, range: UsageRange) -> Result<UsageSummary, KimiError> {
        Ok(crate::usage::query::summary(self.inner.pool(), Some(Platform::Kimi), range).await?)
    }

    pub async fn usage_by_project(&self, range: UsageRange) -> Result<Vec<UsageGroup>, KimiError> {
        Ok(
            crate::usage::query::usage_by_project(self.inner.pool(), Some(Platform::Kimi), range)
                .await?,
        )
    }

    pub async fn usage_by_model(&self, range: UsageRange) -> Result<Vec<UsageGroup>, KimiError> {
        Ok(
            crate::usage::query::usage_by_model(self.inner.pool(), Some(Platform::Kimi), range)
                .await?,
        )
    }

    /// 用量趋势时间序列（Day / Last5Hours 按小时、Week / Month 按日分桶）
    pub async fn usage_series(&self, range: UsageRange) -> Result<UsageSeries, KimiError> {
        Ok(
            crate::usage::query::usage_series(self.inner.pool(), Some(Platform::Kimi), range)
                .await?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone, Utc};
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
            .execute(store.inner.pool())
            .await
            .unwrap();
        let report = store.scan_and_record(tmp.path()).await.unwrap();
        assert_eq!(report.events_added, 0);
        assert_eq!(report.files_rescanned, 0);
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_daily")
            .fetch_one(store.inner.pool())
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
