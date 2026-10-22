# 首页看板维护指南

[返回 AGENTS 索引](../../AGENTS.md) · [模块接入规范](../03-模块开发指南.md) · [应用壳](app-shell.md)

## 当前实现

首页 `/dashboard` 默认由根路由重定向进入。`Dashboard.vue` 展示标题和响应式网格，通过 `dashboardModules` 遍历模块自有摘要组件，并由 `ModuleCardBoundary` 包裹实现卡片级错误隔离。当前四张卡已登记：魔法工具、网络诊断、系统状态、空间维护。

看板没有自己的 API 或 Store，没有全局轮询，不执行 mihomo 查询或系统扫描。诊断与扫描类操作在卡片上由用户点击触发，挂载只做只读状态读取。

## 文件与数据链

- `apps/desktop/src/app/router.ts`：固定首页路由与根重定向。
- `apps/desktop/src/app/modules.ts`：业务模块可选 `dashboard` 组件及过滤后的 `dashboardModules`。
- `apps/desktop/src/modules/dashboard/Dashboard.vue`：布局和组件挂载。
- `apps/desktop/src/modules/dashboard/components/ModuleCardBoundary.vue`：卡片级错误边界，`onErrorCaptured` 只降级当前卡。
- 各模块 `components/OverviewCard.vue`：摘要卡实现，复用所属模块 API/状态。

链路：看板布局 → 模块摘要卡 → 所属 Store/API → 模块后端。不得让 Dashboard 导入各模块 Store 后再次计算状态。

## 生命周期与错误

卡片由 `defineAsyncComponent` 加载；`ModuleCardBoundary` 用 `onErrorCaptured` 捕获单卡的异步加载失败与渲染异常，降级为标题 + 错误信息 + 重试按钮，其余卡片继续渲染（错误对象仍输出到控制台）。重试递增 key 重建卡内容，触发异步组件重新加载。模块自身的业务错误（IPC 失败）由卡片内的 `LoadErrorBanner` 展示，同样不外溢。

看板只读取摘要快照；进入详情后的轮询由对应模块管理。新增卡片时避免挂载即测速、诊断或扫描；一次状态读取和主动操作必须区分。

## 修改指引

新增摘要卡先在所属模块实现 `components/OverviewCard.vue`，复用该模块事实源，在 `toolModules` 中登记。只有存在真实摘要时才登记，不加空卡片占位。卡片提供模块入口和明确的未配置/未检查/失败状态；不因缓存值存在就声称是实时结果。

卡片级错误边界与四张摘要卡已实现并通过 Vitest 行为测试（`src/modules/dashboard/dashboard.spec.ts`）验证；真实桌面下的降级与重试表现仍待人工验收。布局调整不应绕过唯一登记表硬编码模块列表。

## 验证

```powershell
pnpm --filter toogl-desktop test src/modules/dashboard/dashboard.spec.ts src/app/navigation.spec.ts
pnpm build
```

人工验证：现有窗口打开看板，确认当前魔法工具摘要与详情口径一致；mihomo 不可达时查看降级说明，点击入口应到 `/proxy`。新增卡片后还须验证一张卡的失败不会破坏其余卡片。本轮未启动桌面窗口验证。
