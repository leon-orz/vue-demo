# 系统状态模块维护指南

[返回 AGENTS 索引](../../AGENTS.md) · [模块接入规范](../03-模块开发指南.md) · [空间维护](storage.md) · [基础设施](infrastructure.md)

## 当前实现与边界

读取 Windows 版本/显示版本/内部版本、架构、设备名、处理器、物理内存和系统盘可用/总空间。版本及处理器来自 HKLM 只读键，内存使用 `GlobalMemoryStatusEx`，系统盘容量用 `GetDiskFreeSpaceExW`，系统目录通过 `GetSystemDirectoryW` 定位。

页面挂载时读取，可手动刷新；空间维护页面也使用同一个 composable 读取系统盘比例。所有结果只在当前页面实例内存中，不持久化、不后台轮询。未实现温度、电池、GPU、进程管理、注册表优化或自动监控。

## 文件职责与调用链

- `SystemStatus.vue`：装配只读信息面板，挂载时读取一次。
- `useSystemInformation.ts`：快照、结构化错误、加载锁、系统盘比例和读取动作；空间页共享相同 composable 实现。
- `api.ts`：`getSystemInformation` → `invoke('system_tools_information')`。
- `components/SystemInformationPanel.vue`：信息展示、空/加载/失败状态和手动刷新。
- `apps/desktop/src-tauri/src/commands/system_tools.rs`：IPC 薄层，将同步查询放入 `spawn_blocking`。
- `crates/core/src/system_tools.rs`：Windows API、注册表查询和 DTO 组装。
- `crates/core/src/models/system_tools.rs`：`SystemInformation` 序列化模型。

链路：页面挂载/刷新 → composable → API → Tauri command → `spawn_blocking(toogl_core::system_tools::system_information)` → Windows API/注册表 → Rust 模型 → ts-rs bindings → 页面。

## 事实源与生命周期

成功调用返回只读快照；系统盘比例由 free/total 两个 bigint 计算，仅用于展示，总量为零时显示空状态。读取成功替换快照，读取失败保留旧快照并展示错误。请求在途时忽略重复刷新；不存在页面轮询。

## 失败与安全边界

- 必要注册表键无法打开、内存或磁盘 API 失败时返回 `CommandError`，不伪造完整默认对象。
- 注册表键能打开但值缺失时，部分字符串字段当前退化为空字符串；不要把它误称为读取失败。
- 系统盘从 Windows 系统目录定位，不信任进程可覆盖的 `SystemDrive` 环境变量。
- 该链路只读，不修改注册表、文件、代理、网络或进程。
- Tauri command 不含采集算法，只负责阻塞线程调度和错误映射。

## 修改入口

新增模型字段先改 `crates/core/src/models/system_tools.rs` 并运行 bindings 导出测试，再更新 API 使用方和展示。修改 Windows API/字段来源/错误边界改 core 实现并覆盖测试。修改页面展示改 panel；修改请求状态改 composable。空间维护需继续复用此 composable，不复制系统盘查询。

## 验证

维护者定向运行：

```powershell
cargo test -p toogl-core system_tools
pnpm --filter toogl-desktop test src/modules/settings/store.spec.ts
```

模型变化后运行 `cargo test -p toogl-core export_bindings`。全量门禁遵循仓库 CI。

人工验收：进入系统状态确认读取结果和刷新行为；若查询失败，确认最近快照仍显示且错误可重试；从空间维护确认系统盘信息来自同一实现。没有在真实窗口验证的行为不得记作已验收。
