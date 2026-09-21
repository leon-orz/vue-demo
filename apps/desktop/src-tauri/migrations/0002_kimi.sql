-- M1 Kimi 统计（《02-开发计划》M1 数据表要点）
-- 注意：crates/core/src/kimicode/store.rs 以 include_str! 复用本文件建测试库，改动需同步跑 cargo test

-- Kimi 开放平台账户（单行配置；credential_ref 是 keyring 里的 key，凭据本身不落库）
CREATE TABLE IF NOT EXISTS kimi_account (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    name TEXT NOT NULL DEFAULT 'Kimi 开放平台',
    api_base TEXT NOT NULL DEFAULT 'https://api.moonshot.cn',
    credential_ref TEXT NOT NULL DEFAULT 'kimi-openplatform-api-key',
    enabled INTEGER NOT NULL DEFAULT 1
);

-- wire.jsonl 增量解析游标：offset 之后的部分下次续读；文件变短则重扫
CREATE TABLE IF NOT EXISTS kimi_log_file (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    offset INTEGER NOT NULL DEFAULT 0,
    size INTEGER NOT NULL DEFAULT 0,
    mtime INTEGER NOT NULL DEFAULT 0,
    last_scanned_at TEXT NOT NULL
);

-- 用量事件：只追加；文件截断重扫时按 log_file_id 整批删除后重建
CREATE TABLE IF NOT EXISTS kimi_usage_event (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    log_file_id INTEGER NOT NULL REFERENCES kimi_log_file(id) ON DELETE CASCADE,
    ts INTEGER NOT NULL,
    session_id TEXT NOT NULL,
    project TEXT NOT NULL,
    model TEXT NOT NULL,
    input_other INTEGER NOT NULL DEFAULT 0,
    output INTEGER NOT NULL DEFAULT 0,
    cache_read INTEGER NOT NULL DEFAULT 0,
    cache_creation INTEGER NOT NULL DEFAULT 0
);

-- 最近 5 小时窗口查询走事件表
CREATE INDEX IF NOT EXISTS idx_kimi_usage_event_ts
    ON kimi_usage_event (ts);

-- 按日预聚合：扫描后幂等重算，页面查询只打本表
CREATE TABLE IF NOT EXISTS kimi_usage_daily (
    date TEXT NOT NULL,
    project TEXT NOT NULL,
    model TEXT NOT NULL,
    input_other INTEGER NOT NULL DEFAULT 0,
    output INTEGER NOT NULL DEFAULT 0,
    cache_read INTEGER NOT NULL DEFAULT 0,
    cache_creation INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, project, model)
);

-- 开放平台余额历史快照（只追加）
CREATE TABLE IF NOT EXISTS kimi_balance_snapshot (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    available REAL NOT NULL,
    voucher REAL NOT NULL,
    cash REAL NOT NULL,
    fetched_at TEXT NOT NULL
);
