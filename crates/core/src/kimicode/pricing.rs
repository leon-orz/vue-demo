//! Kimi Code 用量估值单价表（M1-09）。
//! 单价来源待定（见《02-开发计划》待定项 1），先内置静态表，字段留接口以后可配置。

/// 四类 token 的单价（元 / 百万 token）。
// TODO(S1-2 后续)：单价数值待官方价格确认后填入，并迁移为 M6 可配置项
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenPriceTable {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_creation_per_mtok: f64,
}

/// 内置单价表。数值未定，暂为 0.0 占位，不做虚假精确。
pub const BUILTIN_PRICES: TokenPriceTable = TokenPriceTable {
    input_per_mtok: 0.0,
    output_per_mtok: 0.0,
    cache_read_per_mtok: 0.0,
    cache_creation_per_mtok: 0.0,
};

/// 按内置单价表估算金额（元）。单价为 0 时恒返回 0。
pub fn estimate_cost(input_other: i64, output: i64, cache_read: i64, cache_creation: i64) -> f64 {
    let p = BUILTIN_PRICES;
    (input_other as f64 * p.input_per_mtok
        + output as f64 * p.output_per_mtok
        + cache_read as f64 * p.cache_read_per_mtok
        + cache_creation as f64 * p.cache_creation_per_mtok)
        / 1_000_000.0
}
