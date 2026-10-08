pub mod error;
pub mod proxy;
pub mod subscription;
pub mod sysproxy;
pub mod usage;

pub use error::{CommandError, CommandErrorKind};
pub use proxy::{
    DelayStatus, MihomoVersion, NodeDelayResult, ProxiesSummary, ProxyGroup, ProxyGroupType,
    ProxyNode, ProxySelection, ProxyStatus, RuleMode,
};
pub use subscription::{ImportResult, SubscriptionInfo};
pub use sysproxy::{SystemProxyCheck, SystemProxyStatus};
pub use usage::{
    LogFileCursor, Platform, PlatformAccountInfo, PlatformAccountInput, PlatformFilter,
    PlatformScanReport, QuotaSnapshot, ScanReport, UsageEvent, UsageGroup, UsagePoint, UsageRange,
    UsageSeries, UsageSummary, UsageTotals,
};

#[cfg(test)]
mod tests {
    /// bindings 完整性：packages/core-types/bindings/ 下每个生成文件都必须在
    /// index.ts 里有对应 re-export（防止新增模型忘记登记；CI 另有 git diff 校验生成物）
    #[test]
    fn bindings_index_covers_all_generated_files() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packages/core-types/bindings"
        );
        let index =
            std::fs::read_to_string(format!("{dir}/index.ts")).expect("无法读取 bindings/index.ts");
        for entry in std::fs::read_dir(dir).expect("无法读取 bindings 目录") {
            let name = entry.unwrap().file_name().into_string().unwrap();
            if name.ends_with(".ts") && name != "index.ts" {
                let stem = name.strip_suffix(".ts").unwrap();
                assert!(
                    index.contains(&format!("'./{stem}'")),
                    "index.ts 缺少对 {name} 的 re-export"
                );
            }
        }
    }
}
