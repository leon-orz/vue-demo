-- M1 小迭代：usage_daily 预聚合表增加请求数（事件计数）列。
-- request_count = 该（date, platform, cli, project, model）桶内 usage_event 行数，
-- 与扫描后的日聚合重算（logs/store.rs）保持同一口径。
-- 幂等说明：ALTER TABLE ADD COLUMN 无 IF NOT EXISTS，由 db.rs 的列守卫
-- （pragma_table_info 检查 request_count 是否已存在）保证重复执行安全；
-- 回填用「全删后按事件表重建」，本身天然幂等（重扫触发日聚合重算时也会刷新该列）。
ALTER TABLE usage_daily ADD COLUMN request_count INTEGER NOT NULL DEFAULT 0;

DELETE FROM usage_daily;

INSERT INTO usage_daily
    (date, platform, cli, project, model,
     input_other, output, cache_read, cache_creation, request_count)
SELECT date(ts / 1000, 'unixepoch', 'localtime'), platform, cli, project, model,
       SUM(input_other), SUM(output), SUM(cache_read), SUM(cache_creation), COUNT(*)
FROM usage_event
GROUP BY 1, 2, 3, 4, 5;
