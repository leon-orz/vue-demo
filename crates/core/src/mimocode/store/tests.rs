//! MimoStore 的 fixture 驱动测试：用 sqlx 在临时目录造 mimocode.db 快照，
//! 覆盖正常事件、缺字段、损坏 JSON 行、增量续读、新增行发现、库重建与降级路径。

use super::*;

/// 按《02-开发计划》M1 节描述造 fixture 库：
/// message 表含 data JSON 字段（tokens / cost / modelID / providerID），rowid 自增。
async fn make_fixture(path: &Path, data_rows: &[String]) {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await.unwrap();
    sqlx::query(
        "CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT,
            time_created INTEGER,
            data TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    for (i, data) in data_rows.iter().enumerate() {
        sqlx::query("INSERT INTO message (id, session_id, time_created, data) VALUES (?, ?, ?, ?)")
            .bind(format!("msg_{i}"))
            .bind("ses_x")
            .bind(0i64)
            .bind(data)
            .execute(&pool)
            .await
            .unwrap();
    }
    pool.close().await;
}

fn usage_json(
    session: &str,
    model: &str,
    ts: i64,
    input: i64,
    output: i64,
    cost: Option<f64>,
) -> String {
    serde_json::json!({
        "sessionID": session,
        "role": "assistant",
        "modelID": model,
        "providerID": "xiaomi",
        "cost": cost,
        "tokens": {"input": input, "output": output, "reasoning": 0,
                   "cache": {"read": 0, "write": 0}},
        "time": {"created": ts, "completed": ts},
    })
    .to_string()
}

async fn temp_store(tmp: &tempfile::TempDir) -> MimoStore {
    let store = MimoStore::connect(tmp.path().join("stats.db"))
        .await
        .unwrap();
    store.migrate().await.unwrap();
    store
}

async fn event_count(store: &MimoStore) -> i64 {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_event WHERE platform = 'mimo'")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    n
}

#[tokio::test]
async fn missing_db_file_returns_empty_report() {
    let tmp = tempfile::tempdir().unwrap();
    let store = temp_store(&tmp).await;
    // 未安装 MiMo Code：文件缺失优雅降级，返回空报告不报错
    let report = store
        .scan_and_record(&tmp.path().join("nonexistent/mimocode.db"))
        .await
        .unwrap();
    assert_eq!(report.files_found, 0);
    assert_eq!(report.files_failed, 0);
    assert_eq!(report.events_added, 0);
}

#[tokio::test]
async fn scan_persists_events_and_daily() {
    let tmp = tempfile::tempdir().unwrap();
    let store = temp_store(&tmp).await;
    let db = tmp.path().join("mimocode.db");
    make_fixture(
        &db,
        &[
            usage_json("ses_1", "mimo-v2", 1000, 100, 10, Some(0.5)),
            // 用户消息（无 tokens）：正常跳过，不是坏行
            serde_json::json!({"role": "user", "time": {"created": 1500}}).to_string(),
            usage_json("ses_2", "mimo-v2", 2000, 200, 20, None),
        ],
    )
    .await;

    let report = store.scan_and_record(&db).await.unwrap();
    assert_eq!(report.files_found, 1);
    assert_eq!(report.events_added, 2);
    assert_eq!(report.bad_lines, 0);

    // (platform, cli, ts, session_id, project, model, cost)
    type EventRow = (String, String, i64, String, String, String, Option<f64>);
    let rows: Vec<EventRow> = sqlx::query_as(
        "SELECT platform, cli, ts, session_id, project, model, cost
         FROM usage_event ORDER BY ts",
    )
    .fetch_all(&store.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "mimo");
    assert_eq!(rows[0].1, "mimocode");
    assert_eq!(rows[0].2, 1000);
    assert_eq!(rows[0].3, "ses_1");
    // 日志无项目来源，统一空串
    assert_eq!(rows[0].4, "");
    assert_eq!(rows[0].5, "mimo-v2");
    // cost 保留（MiMo 套餐进度推算依赖）
    assert_eq!(rows[0].6, Some(0.5));
    assert_eq!(rows[1].6, None);

    // 日聚合已重建
    let (daily_sum,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(input_other), 0) FROM usage_daily WHERE platform = 'mimo'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(daily_sum, 300);

    // 游标记录：offset 为已消费的最大 rowid（3 行，含跳过的用户消息行）
    let (offset,): (i64,) = sqlx::query_as("SELECT offset FROM log_file WHERE platform = 'mimo'")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(offset, 3);

    // 再扫一次：无增量，事件不重复
    let report = store.scan_and_record(&db).await.unwrap();
    assert_eq!(report.events_added, 0);
    assert_eq!(event_count(&store).await, 2);
}

#[tokio::test]
async fn missing_fields_default_and_bad_rows_consumed() {
    let tmp = tempfile::tempdir().unwrap();
    let store = temp_store(&tmp).await;
    let db = tmp.path().join("mimocode.db");
    make_fixture(
        &db,
        &[
            // 缺 cache / cost / modelID / sessionID：按 0 / None / "unknown" 处理
            serde_json::json!({"tokens": {"input": 7, "output": 3}, "time": {"created": 1000}})
                .to_string(),
            // 损坏 JSON：计坏行并跳过
            "{broken".to_string(),
        ],
    )
    .await;

    let report = store.scan_and_record(&db).await.unwrap();
    assert_eq!(report.events_added, 1);
    assert_eq!(report.bad_lines, 1);

    let row: (String, String, i64, i64, Option<f64>) = sqlx::query_as(
        "SELECT session_id, model, cache_read, cache_creation, cost FROM usage_event",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(row.0, "unknown");
    assert_eq!(row.1, "unknown");
    assert_eq!((row.2, row.3), (0, 0));
    assert_eq!(row.4, None);

    // 坏行已消费：再次扫描不再重复计数
    let report = store.scan_and_record(&db).await.unwrap();
    assert_eq!(report.bad_lines, 0);
    assert_eq!(report.events_added, 0);
}

#[tokio::test]
async fn incremental_resume_discovers_new_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let store = temp_store(&tmp).await;
    let db = tmp.path().join("mimocode.db");
    make_fixture(
        &db,
        &[
            usage_json("ses_1", "m1", 1000, 10, 1, None),
            usage_json("ses_1", "m1", 2000, 20, 2, None),
        ],
    )
    .await;
    let report = store.scan_and_record(&db).await.unwrap();
    assert_eq!(report.events_added, 2);

    // 追加新行后再扫：只消费增量部分
    let options = SqliteConnectOptions::new().filename(&db);
    let pool = SqlitePool::connect_with(options).await.unwrap();
    sqlx::query("INSERT INTO message (id, session_id, time_created, data) VALUES ('msg_new', 'ses_1', 0, ?)")
        .bind(usage_json("ses_1", "m1", 3000, 30, 3, Some(0.1)))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let report = store.scan_and_record(&db).await.unwrap();
    assert_eq!(report.events_added, 1);
    assert_eq!(event_count(&store).await, 3);

    let (max_ts,): (i64,) =
        sqlx::query_as("SELECT MAX(ts) FROM usage_event WHERE platform = 'mimo'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(max_ts, 3000);
}

#[tokio::test]
async fn rebuilt_db_rescans_and_replaces_events() {
    let tmp = tempfile::tempdir().unwrap();
    let store = temp_store(&tmp).await;
    let db = tmp.path().join("mimocode.db");
    make_fixture(
        &db,
        &[
            usage_json("ses_1", "m1", 1000, 10, 1, None),
            usage_json("ses_1", "m1", 2000, 20, 2, None),
        ],
    )
    .await;
    store.scan_and_record(&db).await.unwrap();
    assert_eq!(event_count(&store).await, 2);

    // 库被重建（rowid 从 1 重新开始，最大 rowid 小于游标）：从头重扫，旧事件整批替换
    std::fs::remove_file(&db).unwrap();
    make_fixture(&db, &[usage_json("ses_9", "m2", 500, 99, 9, None)]).await;

    let report = store.scan_and_record(&db).await.unwrap();
    assert_eq!(report.files_rescanned, 1);
    assert_eq!(report.events_added, 1);
    assert_eq!(event_count(&store).await, 1);
    let (model,): (String,) =
        sqlx::query_as("SELECT model FROM usage_event WHERE platform = 'mimo'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(model, "m2");

    // 日聚合同步重建，不残留旧数据
    let (daily_sum,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(input_other), 0) FROM usage_daily WHERE platform = 'mimo'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(daily_sum, 99);
}

#[tokio::test]
async fn missing_message_table_counts_failed() {
    let tmp = tempfile::tempdir().unwrap();
    let store = temp_store(&tmp).await;
    let db = tmp.path().join("mimocode.db");
    // schema 漂移：库存在但没有 message 表
    let options = SqliteConnectOptions::new()
        .filename(&db)
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(options).await.unwrap();
    sqlx::query("CREATE TABLE other (id INTEGER)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let report = store.scan_and_record(&db).await.unwrap();
    assert_eq!(report.files_found, 1);
    assert_eq!(report.files_failed, 1);
    assert_eq!(report.events_added, 0);

    // 游标未写入：下次扫描自然重试
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM log_file WHERE platform = 'mimo'")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn garbage_file_counts_failed() {
    let tmp = tempfile::tempdir().unwrap();
    let store = temp_store(&tmp).await;
    let db = tmp.path().join("mimocode.db");
    std::fs::write(&db, "this is not a sqlite database").unwrap();

    let report = store.scan_and_record(&db).await.unwrap();
    assert_eq!(report.files_found, 1);
    assert_eq!(report.files_failed, 1);
    assert_eq!(report.events_added, 0);
}

#[tokio::test]
async fn concurrent_scans_are_serialized_without_double_counting() {
    let tmp = tempfile::tempdir().unwrap();
    let store = temp_store(&tmp).await;
    let db = tmp.path().join("mimocode.db");
    make_fixture(
        &db,
        &[
            usage_json("ses_1", "m1", 1000, 10, 1, None),
            usage_json("ses_1", "m1", 2000, 20, 2, None),
        ],
    )
    .await;

    // 并发触发两次扫描：互斥锁排队执行，第二次读到游标后无增量
    let (r1, r2) = tokio::join!(store.scan_and_record(&db), store.scan_and_record(&db));
    let (r1, r2) = (r1.unwrap(), r2.unwrap());
    assert_eq!(r1.events_added + r2.events_added, 2);
    assert_eq!(event_count(&store).await, 2);
}
