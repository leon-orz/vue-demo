//! M1 多平台用量聚合的业务编排层：扫描调度 / 聚合查询 / 配额刷新 / 账户配置。
//! Tauri commands 与 hub HTTP 接口都是本模块的薄壳，不写重复业务逻辑。
//!
//! UsageService 持有单一 SQLite 连接池，三个平台的 store（kimi/mimo/omp）经
//! from_pool 共享该池（各自的扫描互斥锁仍在 store 内，防并发双计）。

pub mod account;
pub mod query;
pub mod quota;

use std::path::{Path, PathBuf};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

use crate::keystore::SecretStore;
use crate::kimicode::KimiStore;
use crate::mimocode::MimoStore;
use crate::models::{
    CommandError, Platform, PlatformAccountInfo, PlatformAccountInput, PlatformFilter,
    PlatformScanReport, QuotaSnapshot, UsageGroup, UsageRange, UsageSeries, UsageSummary,
};
use crate::omp::OmpStore;

/// M1 业务服务：一个实例管全部平台的用量统计
#[derive(Debug, Clone)]
pub struct UsageService {
    pool: SqlitePool,
    kimi: KimiStore,
    mimo: MimoStore,
    omp: OmpStore,
}

impl UsageService {
    /// 打开（必要时创建）统计数据库并执行 schema 迁移，WAL 模式
    pub async fn connect(db_path: impl AsRef<Path>) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(db_path.as_ref())
            .create_if_missing(true)
            .pragma("journal_mode", "WAL")
            .pragma("busy_timeout", "5000");
        let pool = SqlitePool::connect_with(options).await?;
        crate::db::migrate(&pool).await?;
        Ok(Self {
            kimi: KimiStore::from_pool(pool.clone()),
            mimo: MimoStore::from_pool(pool.clone()),
            omp: OmpStore::from_pool(pool.clone()),
            pool,
        })
    }

    /// 关闭连接池（测试清理临时库文件前调用）
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// 增量扫描日志（scan_usage_logs 命令）：platform 为 None 时扫描全部有日志的平台
    /// （kimi / mimo / omp；deepseek 无本地日志）。单平台失败不影响其他平台，
    /// 各自结果/错误体现在返回值的每个 PlatformScanReport 里。
    /// kimi_sessions_dir 为 Kimi 日志目录覆盖（M6 设置项，None 走默认发现）。
    pub async fn scan(
        &self,
        platform: Option<Platform>,
        kimi_sessions_dir: Option<&Path>,
    ) -> Vec<PlatformScanReport> {
        let platforms: Vec<Platform> = match platform {
            Some(p) => vec![p],
            None => vec![Platform::Kimi, Platform::Mimo, Platform::Omp],
        };
        let mut results = Vec::with_capacity(platforms.len());
        for p in platforms {
            results.push(self.scan_one(p, kimi_sessions_dir).await);
        }
        results
    }

    async fn scan_one(
        &self,
        platform: Platform,
        kimi_sessions_dir: Option<&Path>,
    ) -> PlatformScanReport {
        let ok = |report| PlatformScanReport {
            platform,
            report: Some(report),
            error: None,
        };
        let fail = |msg: String| PlatformScanReport {
            platform,
            report: None,
            error: Some(msg),
        };
        match platform {
            Platform::Kimi => {
                // 日志目录：M6 覆盖优先，其次默认发现（KIMI_CODE_HOME 或 ~/.kimi-code/sessions）
                let dir: PathBuf = match kimi_sessions_dir {
                    Some(d) => d.to_path_buf(),
                    None => match crate::kimicode::default_sessions_dir() {
                        Ok(d) => d,
                        Err(e) => return fail(e.to_string()),
                    },
                };
                match self.kimi.scan_and_record(&dir).await {
                    Ok(r) => ok(r),
                    Err(e) => fail(e.to_string()),
                }
            }
            Platform::Mimo => match crate::mimocode::default_db_path() {
                // 文件缺失由 MimoStore 内部降级为空报告（未安装 MiMo Code 是常态）
                Ok(db) => match self.mimo.scan_and_record(&db).await {
                    Ok(r) => ok(r),
                    Err(e) => fail(e.to_string()),
                },
                Err(e) => fail(e.to_string()),
            },
            Platform::Omp => match crate::omp::default_sessions_dir() {
                Ok(dir) => match self.omp.scan_and_record(&dir).await {
                    Ok(r) => ok(r),
                    Err(e) => fail(e.to_string()),
                },
                Err(e) => fail(e.to_string()),
            },
            Platform::Deepseek => fail("DeepSeek 无本地日志（仅官方余额接口）".to_string()),
        }
    }

    // ---- 聚合查询（platform='all' 为跨平台合计） ----

    pub async fn summary(
        &self,
        platform: PlatformFilter,
        range: UsageRange,
    ) -> Result<UsageSummary, CommandError> {
        query::summary(&self.pool, platform.platform(), range)
            .await
            .map_err(CommandError::from)
    }

    pub async fn usage_by_project(
        &self,
        platform: PlatformFilter,
        range: UsageRange,
    ) -> Result<Vec<UsageGroup>, CommandError> {
        query::usage_by_project(&self.pool, platform.platform(), range)
            .await
            .map_err(CommandError::from)
    }

    pub async fn usage_by_model(
        &self,
        platform: PlatformFilter,
        range: UsageRange,
    ) -> Result<Vec<UsageGroup>, CommandError> {
        query::usage_by_model(&self.pool, platform.platform(), range)
            .await
            .map_err(CommandError::from)
    }

    pub async fn usage_series(
        &self,
        platform: PlatformFilter,
        range: UsageRange,
    ) -> Result<UsageSeries, CommandError> {
        query::usage_series(&self.pool, platform.platform(), range)
            .await
            .map_err(CommandError::from)
    }

    // ---- 配额 / 账户 ----

    /// 刷新平台配额/余额并写快照（编排在 quota 模块）
    pub async fn quota_refresh(
        &self,
        platform: Platform,
        secrets: &dyn SecretStore,
    ) -> Result<QuotaSnapshot, CommandError> {
        quota::refresh(&self.pool, secrets, platform).await
    }

    /// 最近一条配额快照（不触发网络请求）
    pub async fn quota_latest(
        &self,
        platform: Platform,
    ) -> Result<Option<QuotaSnapshot>, CommandError> {
        account::latest_quota_snapshot(&self.pool, platform)
            .await
            .map_err(CommandError::from)
    }

    /// 读取平台账户配置（不含凭据本体，只有 has_credential 标记）
    pub async fn account_get(
        &self,
        platform: Platform,
        secrets: &dyn SecretStore,
    ) -> Result<Option<PlatformAccountInfo>, CommandError> {
        account::get_info(&self.pool, secrets, platform).await
    }

    /// 保存平台账户配置；api_key 非空时写入 keyring（不落库），None 表示沿用已有凭据。
    /// 以 platform 参数为准（input.platform 字段为冗余自描述，不一致时以参数为准）。
    pub async fn account_set(
        &self,
        platform: Platform,
        input: &PlatformAccountInput,
        api_key: Option<&str>,
        secrets: &dyn SecretStore,
    ) -> Result<(), CommandError> {
        account::set_row(&self.pool, platform, input).await?;
        if let Some(key) = api_key.filter(|k| !k.is_empty()) {
            let cred_ref = account::default_credential_ref(platform).ok_or_else(|| {
                CommandError::unavailable(format!("{platform} 平台不支持 API 凭据"))
            })?;
            secrets.set(cred_ref, key)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keystore::MemoryStore;
    use std::io::Write;

    async fn temp_service(tmp: &tempfile::TempDir) -> UsageService {
        UsageService::connect(tmp.path().join("stats.db"))
            .await
            .unwrap()
    }

    /// 造一个 kimi wire.jsonl 会话
    fn make_kimi_session(root: &Path, wd: &str, session: &str, cwd: &str, lines: &[String]) {
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

    fn kimi_usage_line(ts: i64, input: i64) -> String {
        format!(
            r#"{{"type":"usage.record","model":"m1","usage":{{"inputOther":{input},"output":1,"inputCacheRead":0,"inputCacheCreation":0}},"usageScope":"turn","time":{ts}}}"#
        )
    }

    #[tokio::test]
    async fn scan_all_platforms_isolates_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let service = temp_service(&tmp).await;
        let now = chrono::Utc::now().timestamp_millis();
        make_kimi_session(
            tmp.path(),
            "wd_app",
            "s1",
            "C:/work/app",
            &[kimi_usage_line(now, 100)],
        );

        // 全平台扫描：kimi 用覆盖目录扫到 1 条；mimo/omp 的默认路径在本机可能不存在
        // （缺失时降级为空报告），无论如何都不应报错中断其他平台
        let results = service.scan(None, Some(tmp.path())).await;
        assert_eq!(results.len(), 3);
        let kimi = results
            .iter()
            .find(|r| r.platform == Platform::Kimi)
            .unwrap();
        assert_eq!(kimi.error, None);
        assert_eq!(kimi.report.as_ref().unwrap().events_added, 1);
        for r in &results {
            assert!(r.error.is_none(), "{} 不应失败: {:?}", r.platform, r.error);
        }

        // 跨平台合计能看到 kimi 的事件
        let summary = service
            .summary(PlatformFilter::All, UsageRange::Day)
            .await
            .unwrap();
        assert_eq!(summary.totals.input_other, 100);
    }

    #[tokio::test]
    async fn scan_single_platform_and_deepseek_error_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let service = temp_service(&tmp).await;

        // 指定 deepseek：无本地日志，返回错误条目而不 panic
        let results = service.scan(Some(Platform::Deepseek), None).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].platform, Platform::Deepseek);
        assert!(results[0].report.is_none());
        assert!(results[0].error.as_deref().unwrap().contains("无本地日志"));
    }

    #[tokio::test]
    async fn account_set_get_via_service() {
        let tmp = tempfile::tempdir().unwrap();
        let service = temp_service(&tmp).await;
        let secrets = MemoryStore::default();

        let input = PlatformAccountInput {
            platform: Platform::Kimi,
            name: "主账号".to_string(),
            api_base: None,
            quota_total: None,
            quota_cycle: None,
            enabled: true,
        };
        service
            .account_set(Platform::Kimi, &input, Some("sk-abc"), &secrets)
            .await
            .unwrap();
        let info = service
            .account_get(Platform::Kimi, &secrets)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(info.name, "主账号");
        assert!(info.has_credential);
        // 凭据本体进 keyring，不落库
        assert_eq!(
            secrets
                .get(crate::kimicode::KIMI_CREDENTIAL_REF)
                .unwrap()
                .as_deref(),
            Some("sk-abc")
        );

        // 不传 api_key：沿用已有凭据
        service
            .account_set(Platform::Kimi, &input, None, &secrets)
            .await
            .unwrap();
        assert_eq!(
            secrets
                .get(crate::kimicode::KIMI_CREDENTIAL_REF)
                .unwrap()
                .as_deref(),
            Some("sk-abc")
        );

        // mimo 无需凭据：传了 api_key 报 unavailable
        let err = service
            .account_set(Platform::Mimo, &input, Some("x"), &secrets)
            .await
            .unwrap_err();
        assert_eq!(err.kind, "unavailable");
    }
}
