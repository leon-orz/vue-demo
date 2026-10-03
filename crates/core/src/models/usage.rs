//! 用量聚合（M1）数据模型。字段与《02-开发计划》M1 节数据表设计对齐，
//! 对应 migration 0003 的 log_file / usage_event / usage_daily / quota_snapshot / platform_account。
//! 2026-10-02 决策：M1 当前只统计 Kimi Code，Platform 收缩为单一平台（其他平台待回归时
//! 从 git 历史恢复）；schema 与命令层的 platform 维度保持泛化不动。
//! TS 类型由 ts-rs 生成到 packages/core-types/bindings/，禁止手改生成物。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 用量来源平台。序列化为小写字符串，与数据表 platform 列取值一致。
/// 当前只有 Kimi；枚举保留以便后续平台回归（omp/dsh/MiMo 从 git 历史恢复）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub enum Platform {
    /// Kimi（kimi-code CLI 日志 + 开放平台配额）
    Kimi,
}

impl Platform {
    /// 数据表 platform 列的取值
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Kimi => "kimi",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 用量查询命令的平台过滤：具体平台或 all（跨平台合计，不过滤 platform 列）。
/// 序列化为小写字符串（与《02-开发计划》M1 命令表 platform='all' 的约定一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub enum PlatformFilter {
    /// 跨平台合计
    All,
    Kimi,
}

impl PlatformFilter {
    /// 具体平台转为 Some(Platform)，All 转为 None（查询层不加 platform 过滤）
    pub fn platform(self) -> Option<Platform> {
        match self {
            PlatformFilter::All => None,
            PlatformFilter::Kimi => Some(Platform::Kimi),
        }
    }
}

/// 聚合时间范围（M1-05 / M1-06）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub enum UsageRange {
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
pub struct UsageTotals {
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

impl UsageTotals {
    /// 只负责聚合命中率；estimated_cost 由查询层（usage::query）按单价表填充，
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

/// 某个时间范围的用量汇总（usage_summary 返回值）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct UsageSummary {
    pub range: UsageRange,
    pub totals: UsageTotals,
}

/// 按项目 / 按模型分组的一行（key 为项目名或模型名）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct UsageGroup {
    pub key: String,
    pub totals: UsageTotals,
}

/// 趋势图单个时间桶。bucket 取值：Day / Last5Hours 为 "HH:00"（本地时区），Week / Month 为 "YYYY-MM-DD"
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct UsagePoint {
    pub bucket: String,
    pub totals: UsageTotals,
}

/// 用量趋势时间序列（usage_series 返回值）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct UsageSeries {
    pub range: UsageRange,
    pub points: Vec<UsagePoint>,
}

/// 归一化后的单条用量事件（对应 usage_event 表）。
/// 各平台 parser 的产物统一为此模型：ts 为 Unix 毫秒时间戳；cost 可空——
/// 日志自带成本字段时保留，wire.jsonl 无此字段则为 null，估算成本仍由单价表计算。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct UsageEvent {
    pub platform: Platform,
    /// CLI 来源标识（当前为 "kimi-code"），与 usage_event.cli 列一致
    pub cli: String,
    /// Unix 毫秒时间戳（< 2^53，导出为 number）
    #[ts(type = "number")]
    pub ts: i64,
    pub session_id: String,
    /// 项目名：会话工作目录末级目录名
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
    pub cost: Option<f64>,
}

/// 增量解析游标（对应 log_file 表）：每个平台各自记录其日志文件的读取位置。
/// size/mtime（Unix 毫秒）变化即从头重扫；offset 记录上次消费到的位置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct LogFileCursor {
    pub platform: Platform,
    pub path: String,
    #[ts(type = "number")]
    pub offset: i64,
    #[ts(type = "number")]
    pub size: i64,
    /// Unix 毫秒
    #[ts(type = "number")]
    pub mtime: i64,
    /// RFC 3339 最近扫描时间
    pub last_scanned_at: String,
}

/// 配额/余额历史快照（对应 quota_snapshot 表，只追加）。
/// Kimi 窗口余量与余额统一落此模型；
/// total/used/remaining 均可空（如 Kimi 滚动窗口只有 remaining），平台特有明细放 detail_json。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct QuotaSnapshot {
    pub platform: Platform,
    pub total: Option<f64>,
    pub used: Option<f64>,
    pub remaining: Option<f64>,
    /// 平台特有明细的 JSON 串（如 Kimi 余额的 voucher/cash 分项与币种）
    pub detail_json: Option<String>,
    /// RFC 3339 拉取时间
    pub fetched_at: String,
}

/// 平台账户配置视图（platform_account_get 返回值，对应 platform_account 表）。
/// 凭据本体只进 keyring，此处仅有引用与是否存在标记。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct PlatformAccountInfo {
    #[ts(type = "number")]
    pub id: i64,
    pub platform: Platform,
    pub name: String,
    pub api_base: Option<String>,
    /// keyring 里的引用名，不是凭据本身
    pub credential_ref: Option<String>,
    /// keyring 中是否已存有凭据
    pub has_credential: bool,
    pub quota_total: Option<f64>,
    /// 计费周期（如 "monthly"）
    pub quota_cycle: Option<String>,
    pub enabled: bool,
}

/// 平台账户配置入参（platform_account_set）；凭据单独传参并写入 keyring
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct PlatformAccountInput {
    pub platform: Platform,
    pub name: String,
    pub api_base: Option<String>,
    pub quota_total: Option<f64>,
    pub quota_cycle: Option<String>,
    pub enabled: bool,
}

/// 日志扫描结果报告（scan_usage_logs 返回值）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct ScanReport {
    /// 发现的日志文件总数
    #[ts(type = "number")]
    pub files_found: u64,
    /// 因 size/mtime 变化而从头重扫的文件数（含普通追加触发的重扫，不限于截断/重建）
    #[ts(type = "number")]
    pub files_rescanned: u64,
    /// 读取/取元数据失败而跳过的文件数（游标不动，下次扫描自然重试）
    #[ts(type = "number")]
    pub files_failed: u64,
    /// 本次写入的用量事件行数（重扫会整批删除后重放，重放的行数也计入，非纯新增）
    #[ts(type = "number")]
    pub events_added: u64,
    /// 含不完整尾行的文件数（写入半途的行不消费，下次重扫时覆盖）
    #[ts(type = "number")]
    pub partial_lines_skipped: u64,
    /// 解析失败跳过的行数（诊断指标，不中断扫描）
    #[ts(type = "number")]
    pub bad_lines: u64,
}

/// 单平台扫描结果（scan_usage_logs 返回值元素）：单平台失败不影响其他平台，
/// 失败时 report 为 None、error 带可读原因；成功时 error 为 None。
/// 当前只有 Kimi 一路，泛化结构保留给后续回归的平台。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct PlatformScanReport {
    pub platform: Platform,
    pub report: Option<ScanReport>,
    /// 面向用户的失败原因（如 "Kimi 日志目录不存在"）
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 运行本测试即重新生成 packages/core-types/bindings（CI 校验生成物与仓库一致）
    #[test]
    fn export_bindings() {
        Platform::export().unwrap();
        PlatformFilter::export().unwrap();
        UsageRange::export().unwrap();
        UsageTotals::export().unwrap();
        UsageSummary::export().unwrap();
        UsageGroup::export().unwrap();
        UsagePoint::export().unwrap();
        UsageSeries::export().unwrap();
        UsageEvent::export().unwrap();
        LogFileCursor::export().unwrap();
        QuotaSnapshot::export().unwrap();
        PlatformAccountInfo::export().unwrap();
        PlatformAccountInput::export().unwrap();
        ScanReport::export().unwrap();
        PlatformScanReport::export().unwrap();
    }

    #[test]
    fn platform_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Platform::Kimi).unwrap(), "\"kimi\"");
        assert_eq!(Platform::Kimi.as_str(), "kimi");
    }

    #[test]
    fn platform_filter_serializes_lowercase_and_maps() {
        assert_eq!(
            serde_json::to_string(&PlatformFilter::All).unwrap(),
            "\"all\""
        );
        assert_eq!(PlatformFilter::All.platform(), None);
        assert_eq!(PlatformFilter::Kimi.platform(), Some(Platform::Kimi));
    }

    #[test]
    fn cache_hit_rate_null_when_no_input() {
        let t = UsageTotals::new(0, 100, 0, 0);
        assert_eq!(t.cache_hit_rate, None);
    }

    #[test]
    fn cache_hit_rate_formula() {
        // 命中率只算输入侧：cache_read / (input_other + cache_read + cache_creation)
        let t = UsageTotals::new(30, 999, 60, 10);
        assert_eq!(t.cache_hit_rate, Some(0.6));
    }
}
