# 魔法工具模块（代理核心）

> 辅助理解本模块的功能边界与前后端逻辑。维护口径以 [代理与内核](../../../../docs/modules/proxy.md)、[订阅与导入](../../../../docs/modules/subscription.md) 指南为准。

## 功能

Toogl 的核心模块，管理 mihomo 代理内核的完整闭环：

- **内核生命周期**：启动 mihomo sidecar（端口顺延 + 首跑最小配置生成），启动失败只让本模块降级，不阻塞其他模块；退出时先恢复系统代理再结束 sidecar。
- **状态读取**：轮询聚合状态（sidecar 存活、代理组/节点、规则模式、系统代理、订阅概要）；mihomo 侧是事实源，本地记忆值只作降级展示。
- **节点与模式**：selector 组切换节点、组测速、规则/全局/直连模式切换；乐观更新 + 失败回滚 + 动作后强制刷新对齐事实源。
- **系统代理**：开启前备份原状态到 SQLite，关闭/退出/异常退出（下次启动自检）恢复；controller 只监听回环。
- **订阅**：HTTP 拉取/粘贴分享链接 → 解析合并（手动同名优先）→ 原子写节点文件 → mihomo 热重载 → 失败回滚；24 小时后台更新由 Rust 管理。

## 逻辑链路

```text
Proxy.vue（页面，进入轮询/离开停止）
  ├→ store.ts（useProxyStore：状态轮询、动作、乐观回滚、刷新序号防竞态）
  │    → api.ts → proxy_status / select_proxy / set_system_proxy / …（Tauri 命令）
  ├→ subscription.ts（useSubscriptionStore：订阅链接、手动更新、粘贴导入）
  │    → subscription-api.ts → subscription_* 命令
  └→ components/（ProxyGroupCard 节点列表、Configuration 订阅配置、OverviewCard 看板卡）
        → crates/core：sidecar.rs（内核）、mihomo/client.rs（REST）、sysproxy.rs（系统代理）、
          subscription/（解析与更新）、proxy_setting.rs（持久化）
```

## 关键不变量

- 刷新序号 `refreshSequence`：动作（切换/测速/开关）开始后，动作前发出的旧响应不得覆盖动作后的状态。
- 动作统一模式：防重入 → 记 actionError → 乐观更新 → 调 IPC → `refresh(true)` 对齐 → 失败回滚。
- 轮询成对启停，`document.hidden` 跳过当轮；订阅后台更新不依赖页面打开。
- 残留恢复提示可「忽略」，仅本次会话生效；后端自检标记不因此清除。

## 文件

| 文件                                      | 职责                                                        |
| ----------------------------------------- | ----------------------------------------------------------- |
| `Proxy.vue`                               | 页面装配：状态区、模式切换、节点卡片、订阅配置              |
| `store.ts`                                | 代理状态与动作（`PROXY_POLL_INTERVAL`、`RULE_MODE_LABELS`） |
| `subscription.ts` / `subscription-api.ts` | 订阅状态与 IPC                                              |
| `api.ts`                                  | 代理 IPC 薄映射                                             |
| `components/OverviewCard.vue`             | 看板摘要卡                                                  |
| `components/ProxyGroupCard.vue`           | 代理组节点列表与测速                                        |
| `components/Configuration.vue`            | 订阅链接与粘贴导入                                          |
