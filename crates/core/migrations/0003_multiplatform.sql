-- S1-4 多平台泛化（《02-开发计划》M1 节数据表要点）
-- 本文件由 crates/core/src/db.rs 按分号切分逐条执行，禁止在字符串字面量中使用分号；
-- 全部语句必须幂等（IF NOT EXISTS / DELETE+INSERT / WHERE NOT EXISTS），改动后需同步跑 cargo test

-- 增量解析游标（泛化自 kimi_log_file）：唯一约束由 path 改为 (platform, path)，
-- 同一文件路径可被多个平台 parser 各自记录（实际上不会发生，约束语义以平台隔离为准）
CREATE TABLE IF NOT EXISTS log_file (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    platform TEXT NOT NULL,
    path TEXT NOT NULL,
    offset INTEGER NOT NULL DEFAULT 0,
    size INTEGER NOT NULL DEFAULT 0,
    mtime INTEGER NOT NULL DEFAULT 0,
    last_scanned_at TEXT NOT NULL,
    UNIQUE (platform, path)
);

-- 用量事件：只追加；cost 可空（mimocode/omp 日志自带成本时保留，wire.jsonl 无此字段为 NULL，
-- 估算成本仍由单价表计算）；log_file_id 外键用于文件截断/重扫时整批删除重建
CREATE TABLE IF NOT EXISTS usage_event (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    platform TEXT NOT NULL,
    cli TEXT NOT NULL,
    log_file_id INTEGER NOT NULL REFERENCES log_file(id) ON DELETE CASCADE,
    ts INTEGER NOT NULL,
    session_id TEXT NOT NULL,
    project TEXT NOT NULL,
    model TEXT NOT NULL,
    input_other INTEGER NOT NULL DEFAULT 0,
    output INTEGER NOT NULL DEFAULT 0,
    cache_read INTEGER NOT NULL DEFAULT 0,
    cache_creation INTEGER NOT NULL DEFAULT 0,
    cost REAL
);

-- 最近 5 小时窗口查询走事件表（保留 ts 索引）
CREATE INDEX IF NOT EXISTS idx_usage_event_ts
    ON usage_event (ts);

-- 按日预聚合：扫描后幂等重算（按 platform+cli 维度 DELETE 后重建），页面查询只打本表
CREATE TABLE IF NOT EXISTS usage_daily (
    date TEXT NOT NULL,
    platform TEXT NOT NULL,
    cli TEXT NOT NULL,
    project TEXT NOT NULL,
    model TEXT NOT NULL,
    input_other INTEGER NOT NULL DEFAULT 0,
    output INTEGER NOT NULL DEFAULT 0,
    cache_read INTEGER NOT NULL DEFAULT 0,
    cache_creation INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, platform, cli, project, model)
);

-- 配额/余额历史快照（只追加）：Kimi 窗口余量、DeepSeek 余额、MiMo 推算进度统一落此表；
-- total/used/remaining 均可空（如 Kimi 窗口只有 remaining），平台特有明细放 detail_json
CREATE TABLE IF NOT EXISTS quota_snapshot (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    platform TEXT NOT NULL,
    total REAL,
    used REAL,
    remaining REAL,
    detail_json TEXT,
    fetched_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_quota_snapshot_platform_time
    ON quota_snapshot (platform, fetched_at);

-- 各平台账户配置；credential_ref 是 keyring 里的 key 不是凭据本体；
-- MiMo 无公开配额 API，用 quota_total/quota_cycle 手录套餐总额与计费周期。
-- 注意：0001 的 platform 表是网关时代的通用平台注册表（无 platform 代码列与套餐字段），
-- 已被本表取代；旧表与旧 usage_snapshot 一并保留不 DROP，由后续集成任务统一清理
CREATE TABLE IF NOT EXISTS platform_account (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    platform TEXT NOT NULL,
    name TEXT NOT NULL,
    api_base TEXT,
    credential_ref TEXT,
    quota_total REAL,
    quota_cycle TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    UNIQUE (platform, name)
);

-- 数据回填：kimi_log_file → log_file（platform='kimi'）。
-- 用 WHERE NOT EXISTS 而非 DELETE+INSERT：保留已回填行的 id，usage_event 经 path 关联依赖它
INSERT INTO log_file (platform, path, offset, size, mtime, last_scanned_at)
SELECT 'kimi', k.path, k.offset, k.size, k.mtime, k.last_scanned_at
FROM kimi_log_file k
WHERE NOT EXISTS (
    SELECT 1 FROM log_file l WHERE l.platform = 'kimi' AND l.path = k.path
);

-- 数据回填：kimi_usage_event → usage_event（platform='kimi', cli='kimi-code', cost=NULL）。
-- 新旧 log_file_id 经 path 关联映射；先 DELETE 再插保证重复执行幂等
DELETE FROM usage_event WHERE platform = 'kimi';

INSERT INTO usage_event
    (platform, cli, log_file_id, ts, session_id, project, model,
     input_other, output, cache_read, cache_creation, cost)
SELECT 'kimi', 'kimi-code', l.id, e.ts, e.session_id, e.project, e.model,
       e.input_other, e.output, e.cache_read, e.cache_creation, NULL
FROM kimi_usage_event e
JOIN kimi_log_file k ON k.id = e.log_file_id
JOIN log_file l ON l.platform = 'kimi' AND l.path = k.path;

-- 数据回填：kimi_usage_daily → usage_daily（预聚合结果直接搬运，等价于从事件重算）
DELETE FROM usage_daily WHERE platform = 'kimi' AND cli = 'kimi-code';

INSERT INTO usage_daily
    (date, platform, cli, project, model,
     input_other, output, cache_read, cache_creation)
SELECT date, 'kimi', 'kimi-code', project, model,
       input_other, output, cache_read, cache_creation
FROM kimi_usage_daily;

-- kimi_balance_snapshot（开放平台余额历史）不迁移：历史余额快照价值低，
-- 且余额语义已由 quota_snapshot 承接（后续余额适配器改写新表）；
-- 旧 kimi_* 五表本轮保留不 DROP，命令层切换后由后续集成任务清理
