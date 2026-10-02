//! Kimi 开放平台账户与余额模型（M1）。
//! 聚合/事件模型已泛化迁移至 models::usage（S1-4 多平台泛化）；
//! 本文件只保留 kimi 账户/余额命令仍在使用的模型，后续随 platform_account 统一。
//! TS 类型由 ts-rs 生成到 packages/core-types/bindings/，禁止手改生成物。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
        KimiBalance::export().unwrap();
        KimiAccountInfo::export().unwrap();
        KimiAccountInput::export().unwrap();
    }
}
