//! Kimi 统计（M1）数据模型。字段与《02-开发计划》M1 节数据表设计对齐。
//! TS 类型由 ts-rs 生成到 packages/core-types/bindings/，禁止手改生成物。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 聚合时间范围（M1-05 / M1-06）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub enum KimiUsageRange {
    /// 今天（本地时区零点起）
    Day,
    /// 本周（周一起）
    Week,
    /// 本月（1 号起）
    Month,
    /// 最近 5 小时（近似官方限流滚动窗口，走事件表实时聚合）
    Last5Hours,
}

/// 四类 token 的聚合结果 + 缓存命中率 + 估值金额。
/// 缓存命中率 = cache_read / (input_other + cache_read + cache_creation)，分母为 0 时为 null。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct KimiUsageTotals {
    // i64 数值远小于 2^53，导出为 number 以匹配 serde_json 的实际序列化结果
    #[ts(type = "number")]
    pub input_other: i64,
    #[ts(type = "number")]
    pub output: i64,
    #[ts(type = "number")]
    pub cache_read: i64,
    #[ts(type = "number")]
    pub cache_creation: i64,
    pub cache_hit_rate: Option<f64>,
    /// 按内置静态单价表估算的金额（元）。由查询层填充；单价未定，当前恒为 0（见 kimicode::pricing 的 TODO）
    pub estimated_cost: f64,
}

impl KimiUsageTotals {
    /// 只负责聚合命中率；estimated_cost 由查询层（kimicode::store）按单价表填充，
    /// 数据层不反向依赖业务层。
    pub fn new(input_other: i64, output: i64, cache_read: i64, cache_creation: i64) -> Self {
        let input_total = input_other + cache_read + cache_creation;
        let cache_hit_rate = if input_total > 0 {
            Some(cache_read as f64 / input_total as f64)
        } else {
            None
        };
        Self {
            input_other,
            output,
            cache_read,
            cache_creation,
            cache_hit_rate,
            estimated_cost: 0.0,
        }
    }
}

/// 某个时间范围的用量汇总（kimi_usage_summary 返回值）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct KimiUsageSummary {
    pub range: KimiUsageRange,
    pub totals: KimiUsageTotals,
}

/// 按项目 / 按模型分组的一行（key 为项目名或模型名）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct KimiUsageGroup {
    pub key: String,
    pub totals: KimiUsageTotals,
}

/// 趋势图单个时间桶。bucket 取值：Day / Last5Hours 为 "HH:00"（本地时区），Week / Month 为 "YYYY-MM-DD"
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct KimiUsagePoint {
    pub bucket: String,
    pub totals: KimiUsageTotals,
}

/// 用量趋势时间序列（kimi_usage_series 返回值）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct KimiUsageSeries {
    pub range: KimiUsageRange,
    pub points: Vec<KimiUsagePoint>,
}

/// 归一化后的单条用量事件（对应 kimi_usage_event 表，M1-02）。
/// ts 为 Unix 毫秒时间戳。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct KimiUsageEvent {
    /// Unix 毫秒时间戳（< 2^53，导出为 number）
    #[ts(type = "number")]
    pub ts: i64,
    pub session_id: String,
    /// 项目名：会话 state.json 的 cwd 末级目录名
    pub project: String,
    pub model: String,
    #[ts(type = "number")]
    pub input_other: i64,
    #[ts(type = "number")]
    pub output: i64,
    #[ts(type = "number")]
    pub cache_read: i64,
    #[ts(type = "number")]
    pub cache_creation: i64,
}

/// 增量扫描结果报告（scan_kimi_logs 返回值）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct KimiScanReport {
    /// 发现的 wire.jsonl 文件总数
    #[ts(type = "number")]
    pub files_found: u64,
    /// 因截断/重建/mtime 变化而从头重扫的文件数
    #[ts(type = "number")]
    pub files_rescanned: u64,
    /// 读取/取元数据失败而跳过的文件数（游标不动，下次扫描自然重试）
    #[ts(type = "number")]
    pub files_failed: u64,
    /// 本次新增入库的用量事件数
    #[ts(type = "number")]
    pub events_added: u64,
    /// 含不完整尾行的文件数（写入半途的行不消费，下次续读覆盖）
    #[ts(type = "number")]
    pub partial_lines_skipped: u64,
    /// 解析失败跳过的行数（诊断指标，不中断扫描）
    #[ts(type = "number")]
    pub bad_lines: u64,
}

/// Kimi 开放平台余额（M1-10），单位人民币元
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct KimiBalance {
    pub available: f64,
    pub voucher: f64,
    pub cash: f64,
    /// RFC 3339 拉取时间
    pub fetched_at: String,
}

/// 开放平台账户配置视图（kimi_account_get 返回值）。
/// 凭据本体只进 keyring，此处仅有引用与是否存在标记。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct KimiAccountInfo {
    pub name: String,
    pub api_base: String,
    /// keyring 里的引用名，不是凭据本身
    pub credential_ref: String,
    /// keyring 中是否已存有凭据
    pub has_credential: bool,
    pub enabled: bool,
}

/// 账户配置入参（kimi_account_set）；api_key 单独传参并写入 keyring
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct KimiAccountInput {
    pub name: String,
    pub api_base: String,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        KimiUsageRange::export().unwrap();
        KimiUsageTotals::export().unwrap();
        KimiUsageSummary::export().unwrap();
        KimiUsageGroup::export().unwrap();
        KimiUsagePoint::export().unwrap();
        KimiUsageSeries::export().unwrap();
        KimiUsageEvent::export().unwrap();
        KimiScanReport::export().unwrap();
        KimiBalance::export().unwrap();
        KimiAccountInfo::export().unwrap();
        KimiAccountInput::export().unwrap();
    }

    #[test]
    fn cache_hit_rate_null_when_no_input() {
        let t = KimiUsageTotals::new(0, 100, 0, 0);
        assert_eq!(t.cache_hit_rate, None);
    }

    #[test]
    fn cache_hit_rate_formula() {
        // 命中率只算输入侧：cache_read / (input_other + cache_read + cache_creation)
        let t = KimiUsageTotals::new(30, 999, 60, 10);
        assert_eq!(t.cache_hit_rate, Some(0.6));
    }
}
