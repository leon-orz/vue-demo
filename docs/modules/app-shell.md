# 应用壳与导航维护指南

[返回 AGENTS 索引](../../AGENTS.md) · [模块接入规范](../03-模块开发指南.md) · [看板](dashboard.md) · [基础设施](infrastructure.md)

## 职责与文件

- `apps/desktop/src/main.ts`：安装 Pinia、router，启动主题校准并挂载应用。
- `App.vue`：视口布局；侧栏和主内容独立滚动，不执行模块业务。
- `app/components/AppSidebar.vue`：品牌、工作台/核心工具/系统工具分组和底部设置。
- `app/components/SidebarLink.vue`：统一选中、hover、键盘 focus 和路由链接样式。
- `app/router.ts`：hash 路由；固定 `/dashboard`、`/settings` 和 `/` 重定向。
- `app/modules.ts`：四个业务模块的 ID、标题、图标、路由和可选摘要卡。

六个入口：看板、魔法工具、网络诊断、系统状态、空间维护、设置。旧 `/system-tools` 聚合路由已移除，没有兼容别名。

## 登记与依赖

业务模块登记在 `toolModules`，由 router 装配 route，由侧栏派生业务入口，由看板派生有摘要卡的模块。看板和设置是应用固定入口，不在 `toolModules` 内；文档不能声称所有入口都在同一个数组。

当前侧栏以 `proxy` 作为核心组，其余业务模块为系统工具组；导航高亮为路径精确匹配，query 不影响高亮。若未来引入子路由，应同步更新 active 规则并添加真实导航行为验证，不能只改路由表。

页面通过动态 import 加载。只在多个真实调用方共享时抽组件；基础 UI 位于 `components/ui`，应用专属导航在 `app/components`，不能混入通用 UI。

## 桌面启动与退出

`apps/desktop/src-tauri/src/main.rs` 检查 WebView2；`lib.rs` 装配 PreferenceState、mihomo、代理命令状态和订阅后台任务，注册所有 invoke 命令。一个后端状态不可用时保留错误供对应模块降级，不让其伪装成首次使用。

正常 `RunEvent::Exit` 先恢复系统代理，再关闭 sidecar；恢复错误仍需保留到下次自检路径，不能用“退出清理尽力执行”代替恢复保证。具体代理生命周期参见 [魔法工具](proxy.md)；订阅任务参见 [节点来源](subscription.md)。

## 修改指引

- 新工具：先定义产品任务，再登记业务模块，提供实际页面；不建无业务空层。
- 新 IPC：Rust command、`lib.rs` 注册、前端 `api.ts` 和 Rust 生成模型同步，单纯存在函数不代表 invoke 可用。
- 新后台任务：Rust 装配，明确在途互斥、退出语义和不可用状态，不放页面定时器。
- 改窗口、资源或 CSP：同步 `tauri.conf.json`，必须由真实桌面/生产包验收，不删除 CSP 绕过问题。

## 验证

```powershell
pnpm --filter toogl-desktop test src/app/navigation.spec.ts
pnpm build
cargo clippy --workspace --all-targets -- -D warnings
```

人工验收：在已有窗口用鼠标/键盘依次进入六个入口、检查高亮与独立滚动；从看板进详情后返回；正常退出检查 mihomo 和系统代理。未授权时不启动项目/浏览器，不以构建结果声称视觉已验证。
