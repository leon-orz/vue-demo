# 首页看板模块

> 辅助理解本模块的功能边界与装配逻辑。维护口径以 [产品说明](../../../../../docs/01-产品说明.md) 为准。

## 功能

首页状态摘要：遍历 `dashboardModules`（`app/modules.ts` 登记表中登记了 `dashboard` 组件的模块）渲染各模块摘要卡。看板只做布局，不持有业务公式、不导入模块 Store、不启动轮询。

## 逻辑链路

```text
Dashboard.vue（布局）
  → dashboardModules（app/modules.ts 过滤派生，非硬编码列表）
  → components/ModuleCardBoundary.vue（卡片级错误边界）
  → 各模块 components/OverviewCard.vue（defineAsyncComponent 按需加载）
```

- **卡片级隔离**：`ModuleCardBoundary` 用 `onErrorCaptured` 捕获单卡的异步加载失败或渲染异常，只降级当前卡（标题 + 错误 + 重试按钮），其余卡片继续渲染；错误对象仍打到控制台。重试会递增 key 销毁重建卡内容，触发异步组件重新加载。
- **数据口径**：卡片由所属模块实现，直接复用模块 API/composable，与详情页同一条 IPC；卡片内展示自己的业务错误，不伪装未检查状态。

当前登记卡：魔法工具 + 规划中功能摘要卡（`components/PlannedModulesCard.vue`，由 `app/modules.ts` 的 `plannedModules` 派生）。

## 文件

| 文件                                | 职责                         |
| ----------------------------------- | ---------------------------- |
| `Dashboard.vue`                     | 标题 + 响应式网格 + 边界包裹 |
| `components/ModuleCardBoundary.vue` | 卡片级错误边界与重试         |
| `components/PlannedModulesCard.vue` | 规划中模块占位摘要卡         |
