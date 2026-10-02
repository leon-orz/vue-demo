-- S1-4 收尾：旧 kimi 体系下线
-- 本文件由 crates/core/src/db.rs 按分号切分逐条执行，禁止在字符串字面量中使用分号；
-- 全部语句幂等（IF EXISTS / WHERE NOT EXISTS），改动后需同步跑 cargo test

-- kimi_account 单行配置迁入 platform_account（credential_ref 原样保留，
-- keyring 中既有凭据的引用名 "kimi-openplatform-api-key" 不变，用户已存的 key 不失效）。
-- 已存在 kimi 平台账户时跳过（幂等 + 不覆盖新体系下的配置）。
-- 注：本语句依赖 kimi_account 表存在（由 0002 创建，先于本迁移执行）；
-- 与 0003 的回填语句同理，表被本迁移 DROP 后不可再重放 0003/0004 全文件（迁移登记保证只执行一次）
INSERT INTO platform_account (platform, name, api_base, credential_ref, enabled)
SELECT 'kimi', k.name, k.api_base, k.credential_ref, k.enabled
FROM kimi_account k
WHERE NOT EXISTS (
    SELECT 1 FROM platform_account p WHERE p.platform = 'kimi'
);

-- 旧 kimi 五表：用量数据已在 0003 回填进泛化表，余额历史不迁移（语义已由 quota_snapshot 承接）
DROP TABLE IF EXISTS kimi_account;
DROP TABLE IF EXISTS kimi_log_file;
DROP TABLE IF EXISTS kimi_usage_event;
DROP TABLE IF EXISTS kimi_usage_daily;
DROP TABLE IF EXISTS kimi_balance_snapshot;

-- 0001 的 platform / usage_snapshot 是 new-api 网关时代（M2，已取消）的通用平台注册表与快照表，
-- 已被 platform_account / quota_snapshot 取代，无任何代码引用，一并清理
DROP TABLE IF EXISTS usage_snapshot;
DROP TABLE IF EXISTS platform;
