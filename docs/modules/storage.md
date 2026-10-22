# 空间维护模块维护指南

> 维护范围：`apps/desktop/src/modules/storage/`、复用的 `system-status/useSystemInformation.ts`、`apps/desktop/src-tauri/src/commands/system_tools.rs` 中扫描命令、`crates/core/src/system_tools.rs` 中扫描实现。

## 当前实现/未实现

当前提供两类只读内容：复用系统状态模块显示系统盘剩余空间；扫描当前用户 `%LOCALAPPDATA%\\Temp`，统计普通文件数量和字节数，并记录跳过项目数量。扫描不删除、不修改、不打开文件内容，也不接受前端传入路径。

扫描边界为：根目录先用 `symlink_metadata` 检查，根目录是重解析点时拒绝；子目录和文件若是重解析点则跳过；非重解析目录仅在 canonical root 下继续遍历。扫描最多访问 50,000 个目录项，并在目录枚举和条目处理之间检查 5 秒时间预算；达到任一检查条件时返回已统计子集并增加 `skipped_entries`。同步文件系统调用本身不能被中途取消，因此该预算是协作式提前停止条件，调用方必须把结果视为可能不完整。

“系统优化”面板目前只有指导性文字，不执行启动项、注册表、服务、电源计划、网络或清理动作。未实现删除确认、逐文件清理、任意目录扫描和后台定时扫描。

## 文件职责

- `Storage.vue`：装配系统盘概况和 Temp 扫描两个局部 composable；挂载时读取系统盘信息。
- `useStorageScan.ts`：保存扫描结果、错误和在途锁；防止重复扫描。
- `api.ts`：把无参数扫描 API 映射到 Tauri command。
- `components/CleanupPanel.vue`：展示扫描范围、计数、跳过项和错误。
- `components/OptimizationPanel.vue`：只提供人工检查建议与安全边界文案。
- `apps/desktop/src/modules/system-status/useSystemInformation.ts`：向空间页面提供系统盘只读快照计算，不复制后端查询。
- `apps/desktop/src-tauri/src/commands/system_tools.rs`：通过 `spawn_blocking` 调用同步扫描核心。
- `crates/core/src/system_tools.rs`：解析当前用户 Temp、检查重解析点、遍历、计数和限制。

## 前后端调用链/IPC

扫描按钮 → `useStorageScan.run()` → `scanTemporaryFiles()` → `invoke('system_tools_scan_cleanup')` → Tauri command → `spawn_blocking(toogl_core::system_tools::scan_temp_directory)` → `LOCALAPPDATA/Temp` 安全扫描 → `CleanupScan`。

系统盘链路为 `Storage.vue` → `useSystemInformation()` → `system_tools_information`，与系统状态页面使用同一实现事实源。扫描 IPC 无参数，返回类型来自 Rust models 和 ts-rs 生成 bindings。

## 事实源与生命周期

扫描结果只保存在当前页面 composable 内存中，不持久化、不后台轮询。用户点击才开始；`scanning` 在途锁防止重复请求，完成后释放。成功扫描替换旧结果；失败保留旧扫描结果并显示错误。开始新的扫描只清除错误，不主动清除旧结果，因此扫描期间仍可看到上一次可用快照。系统盘概况仅在空间页面挂载时读取，同样不持久化。

## 失败安全边界

- 缺少 `LOCALAPPDATA`、Temp 不存在、根目录无法访问或根目录为重解析点时返回 `unavailable`，不回退到当前目录、系统 Temp 或任意环境路径。
扫描协作式检查条目上限和时间预算；同步操作系统文件 API 无法中断，因此时间预算不是严格 wall-clock 截止时间。达到检查条件后返回部分结果并增加 `skipped_entries`，不把部分结果描述为完整清理建议。
- 只读取 metadata 的文件大小；不存在删除、移动、写入或执行命令的路径。
- Tauri 命令层只做线程调度和错误映射，不接受路径、命令或脚本参数。

## 修改入口

修改扫描边界、重解析点判定、条目/时间上限或计数语义进入 `crates/core/src/system_tools.rs`，必须同步更新测试和本指南。修改 IPC 名称进入 `api.ts`、Tauri command 和注册点。修改页面状态或失败缓存进入 `useStorageScan.ts`；修改展示进入 `CleanupPanel.vue`。系统盘显示应继续复用 `useSystemInformation.ts`，不要在本模块新增查询。

未来若增加删除能力，必须单独设计逐项确认、逐项结果和失败保留规则；不得把现有扫描结果直接变成静默一键清理。

## 验证命令/人工步骤

维护者定向检查：`cargo test -p toogl-core system_tools`。
维护者全量门禁：`cargo test --workspace`、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`pnpm build`、`pnpm test:front`。
- 人工验收：进入“空间维护”，确认页面仅扫描当前用户 Temp；准备普通文件、嵌套目录和重解析点后确认只统计普通文件、跳过链接且原文件不变；确认扫描期间重复点击不会发起并行请求；扫描失败时确认旧结果不被清空并显示错误；确认页面无删除、注册表清理或服务关闭动作。

## 相关指南

- [仓库协作指南](../../AGENTS.md)
- [模块开发指南](../03-模块开发指南.md)
