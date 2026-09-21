-- 本文件由 crates/core/src/db.rs 按分号切分逐条执行，禁止在字符串字面量中使用分号；
-- 全部语句必须幂等（IF NOT EXISTS）

-- 平台配置：credential_ref 是 keyring 里的 key，凭据本身不落库
CREATE TABLE IF NOT EXISTS platform (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    base_url TEXT NOT NULL,
    credential_ref TEXT,
    quota_per_unit INTEGER NOT NULL DEFAULT 500000,
    enabled INTEGER NOT NULL DEFAULT 1,
    sort INTEGER NOT NULL DEFAULT 0
);

-- 用量快照：只追加不更新，天然是历史
CREATE TABLE IF NOT EXISTS usage_snapshot (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    platform_id INTEGER NOT NULL REFERENCES platform(id) ON DELETE CASCADE,
    balance REAL NOT NULL,
    used REAL NOT NULL,
    currency TEXT NOT NULL,
    fetched_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_usage_snapshot_platform_time
    ON usage_snapshot (platform_id, fetched_at);
