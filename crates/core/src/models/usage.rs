//! 统一用量快照模型（M1）。字段与《02-开发计划》M1 节一致。
//! TS 类型由 ts-rs 生成到 packages/core-types/bindings/，禁止手改生成物。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub enum Currency {
    Cny,
    Usd,
}

/// 各平台适配器的统一输出，适配器内部差异不外泄。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct UsageSnapshot {
    pub platform_id: i64,
    pub balance: f64,
    pub used: f64,
    pub currency: Currency,
    /// RFC 3339 时间戳
    pub fetched_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 运行本测试即重新生成 packages/core-types/bindings（CI 校验生成物与仓库一致）
    #[test]
    fn export_bindings() {
        Currency::export().unwrap();
        UsageSnapshot::export().unwrap();
    }

    #[test]
    fn serde_roundtrip() {
        let snap = UsageSnapshot {
            platform_id: 1,
            balance: 12.5,
            used: 3.25,
            currency: Currency::Usd,
            fetched_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: UsageSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, back);
    }
}
