# 软件设置维护指南

[返回 AGENTS 索引](../../AGENTS.md) · [模块接入规范](../03-模块开发指南.md) · [公共基础设施](infrastructure.md)

## 当前能力与边界

只管理亮色、暗色、跟随系统主题和版本展示。不读取代理或其他业务设置，不提供未实现的开机自启、窗口行为或置顶开关。

## 文件与调用链

- `apps/desktop/src/modules/settings/Settings.vue`：表单、错误提示和保存禁用状态。
- `store.ts`：Pinia 主题状态、初始化复用、系统主题监听与保存事务。
- `api.ts`：调用 `app_preferences_get`、`app_preferences_set`。
- `apps/desktop/src-tauri/src/commands/preferences.rs`：解析数据库路径、构造可降级状态和调用 core。
- `crates/core/src/preferences.rs`：`app` namespace JSON 读写。
- `crates/core/src/models/preferences.rs`：`AppPreferences` 和 `ThemeMode`，通过 ts-rs 导出。

链路：设置页面 → Store → API → PreferenceState → PreferenceStore → `toogl.db` 的 `preference` 表。

## 状态与错误语义

`main.ts` 安装 Pinia 后调用 `init()`：同步尝试应用 localStorage 缓存，异步读取 SQLite，不等待 IPC 才挂载。

- SQLite 是事实源，`toogl.settings.theme` 仅为首屏缓存。
- `initializing` 复用并发初始化；成功后由 `initialized` 避免重复数据库读取。
- 只有 SQLite 确认返回 `None` 且缓存值有效时才迁移缓存；SQLite 读取失败绝不能写默认值覆盖数据库。
- `init()` 每次先移除旧 matchMedia 监听，再注册一个新监听；只有主题值为 system 才跟随系统改变。
- 保存动作由 `saving` 防重入；SQLite 保存成功后才更新主题值和 DOM。数据库保存失败保留原主题及错误，使用户可以重试。
- localStorage 是可选缓存：访问被浏览器拒绝或写入配额耗尽时，数据库读取/写入和主题应用仍必须继续；损失仅为后续首屏加速。

错误路径不可混淆：数据库读/写错误是业务错误，需要显示；localStorage 的安全异常或配额错误不是业务失败，不得阻断主题和页面初始化。

## 修改指引

新增应用偏好先扩展 Rust `AppPreferences`，为旧 JSON 提供 serde 默认值或明确迁移，再生成 bindings 并更新 Store/UI。不要向前端开放任意 namespace，不要将代理、订阅或其他模块配置塞入软件设置。

## 验证

```powershell
pnpm --filter toogl-desktop test src/modules/settings/store.spec.ts
cargo test -p toogl-core preferences
```

前端用例覆盖旧缓存迁移、数据库优先级、并发初始化、监听器清理、数据库保存失败、读取错误不覆盖及缓存被拒绝时仍可使用。人工验收需在真实桌面切换亮/暗/系统并重启确认 SQLite 持久化；本次未启动桌面窗口。
