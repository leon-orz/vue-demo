//! 平台凭据模型。凭据本体只进 keyring，落库的是 keyring 里的引用名。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
#[serde(tag = "kind", content = "value")]
pub enum Credential {
    /// API Key / 访问令牌（如 Kimi Bearer token）
    ApiKey(String),
    /// 登录 Cookie（new-api 系中转平台常见）
    Cookie(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_bindings() {
        Credential::export().unwrap();
    }
}
