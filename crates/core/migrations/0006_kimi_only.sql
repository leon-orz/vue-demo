-- 2026-10-02 决策：M1 用量统计只保留 Kimi Code，其他平台（mimo/omp/deepseek）实现与 UI 已砍
-- （将来回归时从 git 历史恢复）。本迁移清理五张表中的非 kimi 存量数据；
-- schema 结构（platform/cli 列）不动，泛化性保留。无 mimo/omp 专属表（数据都在泛化表里），故只做 DELETE。
-- 本文件由 crates/core/src/db.rs 按分号切分逐条执行，禁止在字符串字面量中使用分号；
-- DELETE 按条件执行，重复执行为 no-op，天然幂等

-- 先删事件：usage_event.log_file_id 外键引用 log_file(id)（ON DELETE CASCADE 依赖连接开启外键，不赌）
DELETE FROM usage_event WHERE platform != 'kimi';

DELETE FROM usage_daily WHERE platform != 'kimi';

DELETE FROM log_file WHERE platform != 'kimi';

DELETE FROM quota_snapshot WHERE platform != 'kimi';

DELETE FROM platform_account WHERE platform != 'kimi';
