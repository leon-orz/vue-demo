-- S1-3 M5 魔法工具：代理模块设置 KV 表
-- 本文件由 crates/core/src/db.rs 按分号切分逐条执行，禁止在字符串字面量中使用分号；
-- 全部语句必须幂等（IF NOT EXISTS）
--
-- 当前用途：记录 mihomo sidecar external-controller 实际端口（端口冲突顺延后的真实值）。
-- 后续项3系统代理状态备份（sysproxy::ProxyBackupState JSON）也复用本表，key 前缀区分。

CREATE TABLE IF NOT EXISTS proxy_setting (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
