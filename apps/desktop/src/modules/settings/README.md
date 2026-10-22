# 软件设置模块

> 辅助理解本模块的功能边界与前后端逻辑。维护口径以 [模块指南](../../../../docs/modules/settings.md) 为准。

## 功能

只管理全局偏好：主题（浅色/深色/跟随系统）和版本信息。不依赖任何业务模块，不开设未实现的开关（开机自启、置顶、TUN 等均不在范围）。

## 逻辑链路

```text
Settings.vue（页面装配）
  → store.ts（useSettingsStore：主题读取/写入）
  → api.ts（app_preferences_get / app_preferences_set）
  → crates/core/src/preferences.rs（PreferenceStore，app namespace，SQLite 事实源）
```

关键行为：

- **首屏不闪烁**：`main.ts` 同步应用 localStorage 缓存主题后立即挂载，再异步从 SQLite 校准。
- **缓存不是事实源**：localStorage 读写包 try/catch——WebView 禁存储或配额耗尽时仍能读取数据库并正常启动。
- **读取失败 ≠ 首次使用**：SQLite 读取错误不写回默认值；缺失记录返回 `null` 才视为首次使用。

## 文件

| 文件           | 职责                         |
| -------------- | ---------------------------- |
| `Settings.vue` | 页面装配：主题选择与版本信息 |
| `store.ts`     | 主题状态、缓存读写容错       |
| `api.ts`       | IPC 薄映射                   |
