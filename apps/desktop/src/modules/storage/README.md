# 空间维护模块

> 辅助理解本模块的功能边界与前后端逻辑。维护口径以 [模块指南](../../../../docs/modules/storage.md) 为准。

## 功能

1. **空间概况**：复用系统状态模块的 `useSystemInformation` 读取系统盘用量，不另建 IPC。
2. **临时文件扫描**：只扫描当前用户 `%LOCALAPPDATA%\Temp`，只统计不删除。根目录为重解析点时拒绝；子项符号链接/重解析点跳过；限制扫描条目数与总时长（5 秒），超时截断并把剩余计入 `skipped_entries`。

## 逻辑链路

```text
Storage.vue（页面装配）
  → useSystemInformation（系统状态模块复用：盘空间，挂载读取一次）
  → useStorageScan.ts（扫描会话内状态：结果/错误/在途锁，防重入）
  → api.ts（system_tools_scan_cleanup）
  → crates/core/src/system_tools.rs（路径固定、重解析点防护、扫描上限）
```

- 扫描是主动操作：页面进入不自动扫描，用户点击才执行；在途时 `scanning` 防重入。
- 不接受前端传入任意路径——路径限定是安全边界。
- 看板摘要卡 `components/OverviewCard.vue`：盘空间挂载读取，扫描仍需点击触发。

## 文件

| 文件                               | 职责             |
| ---------------------------------- | ---------------- |
| `Storage.vue`                      | 页面装配         |
| `useStorageScan.ts`                | 扫描状态与在途锁 |
| `api.ts`                           | IPC 薄映射       |
| `components/OptimizationPanel.vue` | 系统盘空间概况   |
| `components/CleanupPanel.vue`      | 临时文件扫描卡   |
| `components/OverviewCard.vue`      | 看板摘要卡       |
