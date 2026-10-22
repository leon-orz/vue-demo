# 系统状态模块

> 辅助理解本模块的功能边界与前后端逻辑。维护口径以 [模块指南](../../../../docs/modules/system-status.md) 为准。

## 功能

只读展示 Windows 与设备基础信息：系统版本/版本号、架构、设备名、处理器、物理内存、系统盘剩余/总空间。全部来自 Windows API（版本信息不以环境变量为事实源），不提供任何修改系统设置的动作。

## 逻辑链路

```text
SystemStatus.vue（页面装配）
  → useSystemInformation.ts（挂载读取一次，不轮询；错误经 toCommandError 规整）
  → api.ts（system_tools_information）
  → crates/core/src/system_tools.rs（Windows API 读取）
```

- `systemDriveUsage` 计算系统盘剩余比例，供本页和空间维护复用（空间维护不重复建 IPC）。
- 加载失败不清空已有信息；重试由用户主动触发。
- 看板摘要卡 `components/OverviewCard.vue` 挂载即读取一次，与详情页同一条 IPC，不轮询。

## 文件

| 文件                                    | 职责                           |
| --------------------------------------- | ------------------------------ |
| `SystemStatus.vue`                      | 页面装配                       |
| `useSystemInformation.ts`               | 状态与读取动作；被空间维护复用 |
| `api.ts`                                | IPC 薄映射                     |
| `components/SystemInformationPanel.vue` | 操作系统 / 硬件与存储两张卡    |
| `components/OverviewCard.vue`           | 看板摘要卡                     |
