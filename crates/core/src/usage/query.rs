//! 跨平台用量聚合查询（泛化自原 kimicode::store 的查询方法，S1-4）。
//! platform 为 Some(p) 时按 platform 列过滤，None 为跨平台合计（'all'，不过滤）。
//! range 语义与原实现一致：Day/Week/Month 打 usage_daily 预聚合表（按起始日过滤），
//! Last5Hours 走 usage_event 事件表实时聚合。

use chrono::{Datelike, Local, TimeZone, Utc};
use sqlx::sqlite::SqlitePool;

use crate::kimicode::pricing;
use crate::models::{
    Platform, UsageGroup, UsagePoint, UsageRange, UsageSeries, UsageSummary, UsageTotals,
};

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

/// 各范围的 SQL 来源：天/周/月打预聚合表（按起始日过滤），最近 5 小时走事件表
fn range_source(range: UsageRange) -> (Source, i64) {
    match range {
        UsageRange::Last5Hours => {
            let cutoff = Utc::now().timestamp_millis() - 5 * 3600 * 1000;
            (Source::Event, cutoff)
        }
        UsageRange::Day => (Source::Daily, 0),
        UsageRange::Week => (Source::Daily, 1),
        UsageRange::Month => (Source::Daily, 2),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Event,
    Daily,
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

/// 填充估值金额（单价表当前全 0 占位，见 kimicode::pricing 的 TODO；
/// TODO(S1-4 后续)：单价表按平台/模型区分后这里需要带 model/平台维度）
fn with_estimated_cost(mut totals: UsageTotals) -> UsageTotals {
    totals.estimated_cost = pricing::estimate_cost(
        totals.input_other,
        totals.output,
        totals.cache_read,
        totals.cache_creation,
    );
    totals
}

/// 按平台过滤的 WHERE 子句与绑定值：Some(p) 追加 "AND platform = ?"（事件表/聚合表通用）
fn platform_clause(platform: Option<Platform>) -> &'static str {
    match platform {
        Some(_) => " AND platform = ?",
        None => "",
    }
}

async fn totals_where(
    pool: &SqlitePool,
    platform: Option<Platform>,
    range: UsageRange,
) -> Result<UsageTotals, sqlx::Error> {
    let (source, marker) = range_source(range);
    let row: (i64, i64, i64, i64) = if source == Source::Event {
        let sql = format!(
            "SELECT COALESCE(SUM(input_other),0), COALESCE(SUM(output),0),
                    COALESCE(SUM(cache_read),0), COALESCE(SUM(cache_creation),0)
             FROM usage_event WHERE ts >= ?{}",
            platform_clause(platform)
        );
        let mut q = sqlx::query_as(&sql).bind(marker);
        if let Some(p) = platform {
            q = q.bind(p.as_str());
        }
        q.fetch_one(pool).await?
    } else {
        let start = range_start_date(marker);
        let sql = format!(
            "SELECT COALESCE(SUM(input_other),0), COALESCE(SUM(output),0),
                    COALESCE(SUM(cache_read),0), COALESCE(SUM(cache_creation),0)
             FROM usage_daily WHERE date >= ?{}",
            platform_clause(platform)
        );
        let mut q = sqlx::query_as(&sql).bind(start);
        if let Some(p) = platform {
            q = q.bind(p.as_str());
        }
        q.fetch_one(pool).await?
    };
    Ok(with_estimated_cost(UsageTotals::new(
        row.0, row.1, row.2, row.3,
    )))
}

/// 聚合总览（usage_summary 命令）
pub async fn summary(
    pool: &SqlitePool,
    platform: Option<Platform>,
    range: UsageRange,
) -> Result<UsageSummary, sqlx::Error> {
    Ok(UsageSummary {
        range,
        totals: totals_where(pool, platform, range).await?,
    })
}

/// 按项目 / 按模型分组
async fn group_by(
    pool: &SqlitePool,
    platform: Option<Platform>,
    range: UsageRange,
    group: GroupBy,
) -> Result<Vec<UsageGroup>, sqlx::Error> {
    let group_col = group.column();
    let (source, marker) = range_source(range);
    let rows: Vec<(String, i64, i64, i64, i64)> = if source == Source::Event {
        let sql = format!(
            "SELECT {group_col}, SUM(input_other), SUM(output),
                    SUM(cache_read), SUM(cache_creation)
             FROM usage_event WHERE ts >= ?{}
             GROUP BY {group_col} ORDER BY 2 DESC",
            platform_clause(platform)
        );
        let mut q = sqlx::query_as(&sql).bind(marker);
        if let Some(p) = platform {
            q = q.bind(p.as_str());
        }
        q.fetch_all(pool).await?
    } else {
        let start = range_start_date(marker);
        let sql = format!(
            "SELECT {group_col}, SUM(input_other), SUM(output),
                    SUM(cache_read), SUM(cache_creation)
             FROM usage_daily WHERE date >= ?{}
             GROUP BY {group_col} ORDER BY 2 DESC",
            platform_clause(platform)
        );
        let mut q = sqlx::query_as(&sql).bind(start);
        if let Some(p) = platform {
            q = q.bind(p.as_str());
        }
        q.fetch_all(pool).await?
    };
    Ok(rows
        .into_iter()
        .map(|(key, a, b, c, d)| UsageGroup {
            key,
            totals: with_estimated_cost(UsageTotals::new(a, b, c, d)),
        })
        .collect())
}

/// 按项目分组（usage_by_project 命令）
pub async fn usage_by_project(
    pool: &SqlitePool,
    platform: Option<Platform>,
    range: UsageRange,
) -> Result<Vec<UsageGroup>, sqlx::Error> {
    group_by(pool, platform, range, GroupBy::Project).await
}

/// 按模型分组（usage_by_model 命令）
pub async fn usage_by_model(
    pool: &SqlitePool,
    platform: Option<Platform>,
    range: UsageRange,
) -> Result<Vec<UsageGroup>, sqlx::Error> {
    group_by(pool, platform, range, GroupBy::Model).await
}

/// 用量趋势时间序列（usage_series 命令）：
/// Day / Last5Hours 按小时分桶（事件表，本地时区），Week / Month 按日分桶（预聚合表）
pub async fn usage_series(
    pool: &SqlitePool,
    platform: Option<Platform>,
    range: UsageRange,
) -> Result<UsageSeries, sqlx::Error> {
    let rows: Vec<(String, i64, i64, i64, i64)> = match range {
        UsageRange::Day | UsageRange::Last5Hours => {
            let marker = match range {
                UsageRange::Day => local_midnight_ms(),
                UsageRange::Last5Hours => Utc::now().timestamp_millis() - 5 * 3600 * 1000,
                _ => unreachable!(),
            };
            let sql = format!(
                "SELECT strftime('%H:00', ts / 1000, 'unixepoch', 'localtime') AS bucket,
                        SUM(input_other), SUM(output), SUM(cache_read), SUM(cache_creation)
                 FROM usage_event WHERE ts >= ?{}
                 GROUP BY bucket ORDER BY bucket",
                platform_clause(platform)
            );
            let mut q = sqlx::query_as(&sql).bind(marker);
            if let Some(p) = platform {
                q = q.bind(p.as_str());
            }
            q.fetch_all(pool).await?
        }
        UsageRange::Week | UsageRange::Month => {
            let (_, kind) = range_source(range);
            let start = range_start_date(kind);
            let sql = format!(
                "SELECT date AS bucket,
                        SUM(input_other), SUM(output), SUM(cache_read), SUM(cache_creation)
                 FROM usage_daily WHERE date >= ?{}
                 GROUP BY date ORDER BY date",
                platform_clause(platform)
            );
            let mut q = sqlx::query_as(&sql).bind(start);
            if let Some(p) = platform {
                q = q.bind(p.as_str());
            }
            q.fetch_all(pool).await?
        }
    };
    Ok(UsageSeries {
        range,
        points: rows
            .into_iter()
            .map(|(bucket, a, b, c, d)| UsagePoint {
                bucket,
                totals: with_estimated_cost(UsageTotals::new(a, b, c, d)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;

    async fn temp_pool() -> (tempfile::TempDir, SqlitePool) {
        let tmp = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(tmp.path().join("t.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        crate::db::migrate(&pool).await.unwrap();
        (tmp, pool)
    }

    /// 直接插事件（先补 log_file 行满足外键）；daily 聚合按 scanner 同样的口径重算
    async fn add_event(pool: &SqlitePool, platform: &str, cli: &str, ts: i64, input: i64) {
        sqlx::query(
            "INSERT INTO log_file (platform, path, offset, size, mtime, last_scanned_at)
             VALUES (?, ?, 0, 0, 0, '2026-01-01T00:00:00Z')
             ON CONFLICT(platform, path) DO NOTHING",
        )
        .bind(platform)
        .bind(format!("{platform}.log"))
        .execute(pool)
        .await
        .unwrap();
        let (file_id,): (i64,) =
            sqlx::query_as("SELECT id FROM log_file WHERE platform = ? AND path = ?")
                .bind(platform)
                .bind(format!("{platform}.log"))
                .fetch_one(pool)
                .await
                .unwrap();
        sqlx::query(
            "INSERT INTO usage_event
                 (platform, cli, log_file_id, ts, session_id, project, model,
                  input_other, output, cache_read, cache_creation, cost)
             VALUES (?, ?, ?, ?, 's1', 'app', 'm1', ?, 1, 0, 0, NULL)",
        )
        .bind(platform)
        .bind(cli)
        .bind(file_id)
        .bind(ts)
        .bind(input)
        .execute(pool)
        .await
        .unwrap();
        // 重建本平台日聚合
        sqlx::query("DELETE FROM usage_daily WHERE platform = ? AND cli = ?")
            .bind(platform)
            .bind(cli)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO usage_daily (date, platform, cli, project, model,
                 input_other, output, cache_read, cache_creation)
             SELECT date(ts / 1000, 'unixepoch', 'localtime'), platform, cli, project, model,
                    SUM(input_other), SUM(output), SUM(cache_read), SUM(cache_creation)
             FROM usage_event WHERE platform = ? AND cli = ?
             GROUP BY 1, 2, 3, 4, 5",
        )
        .bind(platform)
        .bind(cli)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn all_platforms_totals_sum_across_platforms() {
        let (_tmp, pool) = temp_pool().await;
        let now = Utc::now().timestamp_millis();
        add_event(&pool, "kimi", "kimi-code", now, 100).await;
        add_event(&pool, "mimo", "mimocode", now, 200).await;
        add_event(&pool, "omp", "omp", now, 300).await;

        let all = summary(&pool, None, UsageRange::Day).await.unwrap();
        assert_eq!(all.totals.input_other, 600);

        // 单平台隔离
        let kimi = summary(&pool, Some(Platform::Kimi), UsageRange::Day)
            .await
            .unwrap();
        assert_eq!(kimi.totals.input_other, 100);
        let mimo = summary(&pool, Some(Platform::Mimo), UsageRange::Day)
            .await
            .unwrap();
        assert_eq!(mimo.totals.input_other, 200);
        // 无数据的平台返回全 0 而不是报错
        let ds = summary(&pool, Some(Platform::Deepseek), UsageRange::Day)
            .await
            .unwrap();
        assert_eq!(ds.totals.input_other, 0);
    }

    #[tokio::test]
    async fn group_and_series_respect_platform_filter() {
        let (_tmp, pool) = temp_pool().await;
        let now = Utc::now().timestamp_millis();
        add_event(&pool, "kimi", "kimi-code", now, 100).await;
        add_event(&pool, "omp", "omp", now, 300).await;

        // 跨平台分组：同名 project/model 跨平台合并（group key 不带平台维度）
        let groups = usage_by_project(&pool, None, UsageRange::Day)
            .await
            .unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].totals.input_other, 400);
        let kimi_only = usage_by_project(&pool, Some(Platform::Kimi), UsageRange::Day)
            .await
            .unwrap();
        assert_eq!(kimi_only[0].totals.input_other, 100);

        // Last5Hours 走事件表，平台过滤同样生效
        let last5 = summary(&pool, Some(Platform::Omp), UsageRange::Last5Hours)
            .await
            .unwrap();
        assert_eq!(last5.totals.input_other, 300);

        let series = usage_series(&pool, None, UsageRange::Day).await.unwrap();
        assert_eq!(
            series
                .points
                .iter()
                .map(|p| p.totals.input_other)
                .sum::<i64>(),
            400
        );
        let series_kimi = usage_series(&pool, Some(Platform::Kimi), UsageRange::Month)
            .await
            .unwrap();
        assert_eq!(
            series_kimi
                .points
                .iter()
                .map(|p| p.totals.input_other)
                .sum::<i64>(),
            100
        );
    }
}
