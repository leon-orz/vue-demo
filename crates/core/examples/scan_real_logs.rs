//! 手动验证工具：对本机真实 Kimi Code 日志目录做一次增量扫描并打印聚合统计。
//! 用法：cargo run -p toogl-core --example scan_real_logs [-- <sessions 目录>]
//! 数据写入系统临时目录下的独立 SQLite，结束时删除，不碰应用数据库。

use toogl_core::kimicode::{default_sessions_dir, KimiStore};
use toogl_core::models::KimiUsageRange;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sessions_dir = match std::env::args().nth(1) {
        Some(p) => p.into(),
        None => default_sessions_dir()?,
    };
    println!("扫描目录: {}", sessions_dir.display());

    let db_path = std::env::temp_dir().join("toogl-scan-real-logs-verify.db");
    let store = KimiStore::connect(&db_path).await?;
    store.migrate().await?;

    let report = store.scan_and_record(&sessions_dir).await?;
    println!("扫描报告: {report:?}");

    for range in [
        KimiUsageRange::Day,
        KimiUsageRange::Week,
        KimiUsageRange::Month,
        KimiUsageRange::Last5Hours,
    ] {
        let summary = store.summary(range).await?;
        println!("{range:?}: {:?}", summary.totals);
    }

    println!("--- 按项目（本月，前 10） ---");
    for g in store
        .usage_by_project(KimiUsageRange::Month)
        .await?
        .iter()
        .take(10)
    {
        println!("  {}: {:?}", g.key, g.totals);
    }

    println!("--- 按模型（本月） ---");
    for g in store.usage_by_model(KimiUsageRange::Month).await? {
        println!("  {}: {:?}", g.key, g.totals);
    }

    store.close().await;
    drop(store); // 释放连接池 Arc，让 SQLite 句柄真正关闭
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    println!("临时数据库已清理: {}", db_path.display());
    Ok(())
}
