pub mod error;
pub mod preferences;
pub mod proxy;
pub mod subscription;
pub mod sysproxy;

pub use error::{CommandError, CommandErrorKind};
pub use preferences::{AppPreferences, ThemeMode};
pub use proxy::{
    DelayStatus, MihomoVersion, NodeDelayResult, ProxiesSummary, ProxyGroup, ProxyGroupType,
    ProxyNode, ProxySelection, ProxyStatus, RuleMode,
};
pub use subscription::{ImportResult, SubscriptionInfo};
pub use sysproxy::{SystemProxyCheck, SystemProxyStatus};

#[cfg(test)]
mod tests {
    /// 先显式导出全部模型，再生成包入口；不依赖并行测试或既有文件的执行顺序。
    #[test]
    fn export_bindings_index() {
        use super::*;
        use ts_rs::TS;

        macro_rules! export_types {
            ($($ty:ty),+ $(,)?) => {{
                $(<$ty>::export().unwrap();)+
                vec![$(<$ty>::name()),+]
            }};
        }
        let mut names = export_types![
            AppPreferences,
            CommandError,
            CommandErrorKind,
            DelayStatus,
            ImportResult,
            crate::subscription::content::NodeFailure,
            MihomoVersion,
            NodeDelayResult,
            ProxiesSummary,
            ProxyGroup,
            ProxyGroupType,
            ProxyNode,
            ProxySelection,
            ProxyStatus,
            RuleMode,
            SubscriptionInfo,
            SystemProxyCheck,
            ThemeMode,
        ];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/core-types/bindings");
        names.sort();
        let mut index = String::from("// 本文件由 Rust 类型导出测试生成，请勿手改。\n");
        for name in names {
            index.push_str(&format!("export type {{ {name} }} from './{name}'\n"));
        }
        // 同目录临时文件 + rename，完整性检查不会读到写入一半的入口。
        let temp = dir.join("index.ts.tmp");
        std::fs::write(&temp, index).unwrap();
        std::fs::rename(temp, dir.join("index.ts")).unwrap();
    }
}
