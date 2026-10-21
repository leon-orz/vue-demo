-- 旧账户、日志、用量和网关业务已下线，不再回填或重建旧表。
-- 先删子表再删父表，兼容开启外键检查的旧库及只剩部分表的旧库。
DROP TABLE IF EXISTS kimi_usage_event;
DROP TABLE IF EXISTS kimi_usage_daily;
DROP TABLE IF EXISTS kimi_balance_snapshot;
DROP TABLE IF EXISTS kimi_log_file;
DROP TABLE IF EXISTS kimi_account;
DROP TABLE IF EXISTS usage_event;
DROP TABLE IF EXISTS usage_daily;
DROP TABLE IF EXISTS quota_snapshot;
DROP TABLE IF EXISTS log_file;
DROP TABLE IF EXISTS platform_account;
DROP TABLE IF EXISTS usage_snapshot;
DROP TABLE IF EXISTS platform;
