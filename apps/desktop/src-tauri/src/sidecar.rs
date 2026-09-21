//! mihomo sidecar 进程生命周期管理（S1-3 落地）。
//!
//! TODO(S1-3)：
//! - 将 mihomo Windows 二进制放入 src-tauri/binaries/ 并在 tauri.conf.json 注册 externalBin
//!   （命名规则：mihomo-x86_64-pc-windows-msvc.exe）
//! - 启动时拉起 sidecar，退出时回收；9090 端口冲突则顺延并写入 proxy_setting
//! - RESTful API 客户端实现见 crates/core/src/mihomo/
