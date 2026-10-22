# 网络诊断模块

> 辅助理解本模块的功能边界与前后端逻辑。维护口径以 [模块指南](../../../../docs/modules/network-diagnostics.md) 为准。

## 功能

两个用户动作，均为按需执行、不持久化、不轮询：

1. **运行诊断**：对固定目标 `example.com:443` 先做 DNS 解析（最长 5 秒），成功后再对第一个解析地址做 TCP 连接（最长 3 秒）。DNS 失败则 TCP 不执行。结果只描述该测试目标，不代表整机网络状态。
2. **刷新 DNS 缓存**：调用系统目录（`GetSystemDirectoryW` 获取）下的 `ipconfig.exe /flushdns`，参数固定。不自动提权、不重置网卡、不改动代理。

## 逻辑链路

```text
NetworkDiagnostics.vue（页面装配）
  → useNetworkDiagnostics.ts（会话内状态：结果/错误/在途锁，防重入）
  → api.ts（invoke 一一映射，无参数）
  → system_tools_diagnose_network / system_tools_flush_dns（Tauri 命令）
  → crates/core/src/system_tools.rs（固定目标、固定超时、固定命令）
```

- 诊断在途时 `diagnosing` 锁防重入；诊断失败保留旧结果，只显示新错误。
- 前端不接受任何自定义目标、路径或命令——这是安全边界，不是待办项。
- 看板摘要卡 `components/OverviewCard.vue` 持有自己的会话内结果，不与页面共享实例；挂载不自动诊断。

## 文件

| 文件                          | 职责                                       |
| ----------------------------- | ------------------------------------------ |
| `NetworkDiagnostics.vue`      | 页面装配，绑定面板事件                     |
| `useNetworkDiagnostics.ts`    | 状态与在途锁，错误经 `toCommandError` 规整 |
| `api.ts`                      | IPC 薄映射                                 |
| `components/NetworkPanel.vue` | 诊断与 DNS 刷新两张卡片的展示              |
| `components/OverviewCard.vue` | 看板摘要卡                                 |
