# 应用壳（app/）

> 应用装配层：模块登记、路由、侧栏导航。全局规则见 [AGENTS.md](../../../AGENTS.md)，架构事实见 [架构与数据流](../../../../docs/02-架构与数据流.md)。

## 职责

| 文件                         | 职责                                                                |
| ---------------------------- | ------------------------------------------------------------------- |
| `modules.ts`                 | 业务模块唯一登记点：导航、路由、可选看板卡、规划中描述（`planned`） |
| `router.ts`                  | 固定路由（`/dashboard`、`/settings`）+ 展开业务路由；hash 路由      |
| `App.vue`（上级目录）        | 只装配布局，不持有业务逻辑                                          |
| `components/AppSidebar.vue`  | 侧栏分组：工作台 / 核心工具 / 系统工具 / 规划中 / 设置              |
| `components/SidebarLink.vue` | 单个导航项：active 态、核心徽标、规划中虚线徽标                     |
| `navigation.spec.ts`         | 模块入口行为测试（占位页无 IPC、设置只读偏好等）                    |

## 规则

- 新增/实现模块只改 `modules.ts`，不动 `router.ts` 和侧栏；侧栏按登记表派生（核心 = `proxy`，规划中 = 有 `planned` 字段的模块）。
- `dashboardModules` / `plannedModules` 由登记表过滤派生，禁止在别处维护平行列表。
