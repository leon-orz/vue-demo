# 网络诊断模块维护指南

> 维护范围：`apps/desktop/src/modules/network-diagnostics/`、`apps/desktop/src-tauri/src/commands/system_tools.rs` 中网络命令、`crates/core/src/system_tools.rs` 中网络实现。

## 当前实现/未实现

当前实现两个用户动作：运行固定目标的 DNS/TCP 诊断，以及刷新 Windows DNS 客户端缓存。诊断目标固定为 `example.com:443`：先在最多 5 秒内解析，再对解析得到的第一个地址执行最多 3 秒的 TCP 连接；DNS 失败或超时不会执行 TCP。结果只说明该测试目标，不代表整机或整网状态。

DNS 刷新只执行 Windows 系统目录中的 `ipconfig.exe`，参数固定为 `/flushdns`，不自动提权、不重置网卡、不改变代理设置。系统目录由 `GetSystemDirectoryW` 获取，不能由前端传入路径或命令。

未实现自定义 URL、脚本或命令，未实现网络配置修复、代理重置、网卡重启、持续监控和自动重试。

## 文件职责

- `NetworkDiagnostics.vue`：装配页面局部 composable 和面板事件。
- `useNetworkDiagnostics.ts`：保存诊断/修复结果、错误和在途锁；重复点击直接忽略。
- `api.ts`：把无参数 API 一一映射到 Tauri `invoke`。
- `components/NetworkPanel.vue`：展示边界、结果和结构化错误，不实现业务判断。
- `apps/desktop/src-tauri/src/commands/system_tools.rs`：在 Tauri 命令层编排异步调用；同步 DNS 刷新通过 `spawn_blocking`。
- `crates/core/src/system_tools.rs`：固定目标、超时、TCP 探测和固定系统命令的事实实现。

## 前后端调用链/IPC

诊断链路为：页面按钮 → `useNetworkDiagnostics.diagnose()` → `diagnoseNetwork()` → `invoke('system_tools_diagnose_network')` → Tauri command → `toogl_core::system_tools::diagnose_network()` → Tokio DNS lookup/TCP connect → `NetworkDiagnostic`。

刷新链路为：页面按钮 → `flushDns()` → `flushDnsCache()` → `invoke('system_tools_flush_dns')` → Tauri command → `spawn_blocking(toogl_core::system_tools::flush_dns)` → `GetSystemDirectoryW` 定位 `ipconfig.exe` → `/flushdns` → `NetworkRepairResult` 或 `CommandError`。

IPC 无参数。返回类型来自 Rust models，并由 ts-rs 生成到 `packages/core-types/bindings/`；不得手改 bindings。

## 事实源与生命周期

网络诊断事实源是本次固定探测的 DNS 和 TCP 结果，不持久化、不后台轮询。页面进入时不会自动执行，用户点击才执行。诊断在途时 `diagnosing` 防重入；完成后无论成功失败都会释放锁。成功结果会替换旧结果；诊断失败时旧诊断结果保留，同时显示新的错误。

DNS 刷新是一次性动作，不持久化。动作在途时 `flushingDns` 防重入；开始新动作会清除上一次修复结果，成功或失败后释放锁。失败信息来自结构化 `CommandError`，前端统一经 `toCommandError` 适配。

## 失败安全边界

- DNS 解析失败、超时或没有地址时返回 DNS 失败状态，绝不继续 TCP。
- TCP 只连接解析结果中的第一个 `SocketAddr`，连接失败仅表示该目标不可达。
- DNS 刷新使用固定系统目录和固定参数，禁止把环境变量、前端输入或可执行文件搜索路径当作命令来源。
- 命令启动失败、非零退出码和 Tauri 调度失败都转为错误，不自动提权或执行替代命令。
- 网络诊断不改变系统状态；DNS 刷新只调用 Windows DNS 客户端刷新动作。

## 修改入口

修改 UI 文案、按钮状态或错误展示时进入 `NetworkPanel.vue`。修改 IPC 名称时必须同时核对 `api.ts`、Tauri command 注册和页面调用方。修改探测目标或超时必须同时更新核心常量、测试、文档和人工验收说明；不得增加前端参数。修改系统命令路径只能在 `crates/core/src/system_tools.rs` 进行，并保持 `GetSystemDirectoryW` 与 `/flushdns` 固定约束。

## 验证命令/人工步骤

- Rust 定向测试：`cargo test -p toogl-core system_tools`。
- 全量门禁：维护者按仓库要求运行 `cargo fmt --all -- --check`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`。
- 人工验收：进入“网络诊断”，运行诊断，确认结果仅描述固定目标；断网或阻断 DNS 时确认不显示 TCP 成功；点击刷新 DNS，确认仅执行系统 DNS 刷新并在权限不足时显示失败；连续快速点击确认按钮不会叠加请求。

## 相关指南

- [仓库协作指南](../../AGENTS.md)
- [模块开发指南](../03-模块开发指南.md)
