# 首页看板维护指南

[返回 AGENTS 索引](../../AGENTS.md) · [模块接入规范](../03-模块开发指南.md) · [应用壳](app-shell.md)

## 当前实现

首页 `/dashboard` 默认由根路由重定向进入。`Dashboard.vue` 展示标题和响应式网格，通过 `dashboardModules` 遍历模块自有摘要组件。目前只有魔法工具登记摘要卡；网络诊断、系统状态、空间维护的摘要卡是规划能力，不是已完成能力。

看板没有自己的 API 或 Store，没有全局轮询，不执行 mihomo 查询或系统扫描。

## 文件与数据链

- `apps/desktop/src/app/router.ts`：固定首页路由与根重定向。
- `apps/desktop/src/app/modules.ts`：业务模块可选 `dashboard` 组件及过滤后的 `dashboardModules`。
- `apps/desktop/src/modules/dashboard/Dashboard.vue`：布局和组件挂载。
- `apps/desktop/src/modules/proxy/components/OverviewCard.vue`：当前唯一摘要卡，读取魔法工具 Store。

链路：看板布局 → 模块摘要卡 → 所属 Store/API → 模块后端。不得让 Dashboard 导入各模块 Store 后再次计算状态。

## 生命周期与错误

当前卡片由 `defineAsyncComponent` 加载，模块自己的加载/业务错误由卡片展示。看板目前没有通用的异步加载失败组件或 Vue 渲染异常边界；不能把“所有卡片级异常均隔离”写成现状保证。

看板只读取摘要快照；进入详情后的轮询由对应模块管理。新增卡片时避免挂载即测速、诊断或扫描；一次状态读取和主动操作必须区分。

## 修改指引

新增摘要卡先在所属模块实现 `components/OverviewCard.vue`，复用该模块事实源，在 `toolModules` 中登记。只有存在真实摘要时才登记，不加空卡片占位。卡片提供模块入口和明确的未配置/未检查/失败状态；不因缓存值存在就声称是实时结果。

产品目标中的所有模块卡片、卡片级错误边界仍需后续实现和行为验证。布局调整不应绕过唯一登记表硬编码模块列表。

## 验证

```powershell
pnpm --filter toogl-desktop test src/app/navigation.spec.ts
pnpm build
```

人工验证：现有窗口打开看板，确认当前魔法工具摘要与详情口径一致；mihomo 不可达时查看降级说明，点击入口应到 `/proxy`。新增卡片后还须验证一张卡的失败不会破坏其余卡片。本轮未启动桌面窗口验证。
