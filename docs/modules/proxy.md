# 代理模块维护指南

相关入口：[AGENTS.md](../../AGENTS.md) · [模块开发指南](../03-模块开发指南.md)

## 当前实现

魔法工具的代理链由桌面端前端、Tauri 命令层、mihomo sidecar 和 `crates/core` 组成。sidecar 在应用 setup 阶段创建 `app_data_dir/mihomo/config.yaml`（首次运行才写入模板），以 `-d <work_dir> -f <config>` 启动；`/version` 是就绪和存活探测事实源。启动成功后，`ProxyCoreState` 持有 external-controller 实际端口、鉴权客户端和子进程句柄。

代理端口有两个概念：external-controller（默认 9090~9100，用于 REST API）和 mixed-port（默认 7890~7900，用于系统代理流量）。首跑会探测并写入模板；已有配置优先以配置文件为准，模板配置的 controller 被占用时可顺延并写回。controller secret 首跑随机生成并写入配置及 `proxy_setting`；带模板标记的旧模板缺 secret 时自动补齐，用户手写配置不静默改写。

`proxy_status` 是聚合读取，不保证所有字段同时来自同一时刻：sidecar `/proxies`、`/configs`、系统注册表和 SQLite 记忆值分别独立降级。mihomo 可用时 `/proxies` 是代理组和当前节点事实源，`/configs` 是规则模式事实源；不可达时才使用 `proxy_setting` 的记忆值用于展示。系统代理状态始终直接读取 Windows 注册表。

当前实现了：代理组节点切换、批量测速、规则模式（rule/global/direct）、Windows 系统代理开关、启动残留检测和确认恢复、订阅节点及手动节点接入。未实现且不得从界面或注释暗示为已实现：TUN、PAC、虚拟网卡、进程级代理和远程控制。

## 文件职责

- `apps/desktop/src/modules/proxy/Proxy.vue` 及 `components/`：页面和展示，不保存业务事实。
- `apps/desktop/src/modules/proxy/api.ts`：Tauri command 的一对一调用；类型来自生成 bindings。
- `apps/desktop/src/modules/proxy/store.ts`：轮询、动作锁、乐观显示、失败回滚和动作后事实对齐。刷新请求带序号，动作开始后旧响应不得覆盖动作后的状态。
- `apps/desktop/src-tauri/src/sidecar.rs`：配置模板、端口和 secret 决策、sidecar 启动/就绪/输出 drain/退出回收。
- `apps/desktop/src-tauri/src/commands/proxy.rs`：State 取用、阻塞注册表操作的 `spawn_blocking`、错误映射和退出前恢复钩子。
- `crates/core/src/mihomo/client.rs`：REST API 薄封装、鉴权、超时、代理摘要转换及并发测速。
- `crates/core/src/sysproxy.rs`：注册表抽象、备份状态和开关/恢复事务的串行化；真实 Windows 实现为 `WinRegIo`。
- `crates/core/src/proxy_setting.rs`：`proxy_setting` KV 的键和 SQLite 状态存取。它是降级记忆与现场备份，不替代 mihomo/注册表事实源。
- `crates/core/src/models/proxy.rs`、`sysproxy.rs`：跨语言 DTO 唯一来源，勿手改 bindings。

## 调用链与 IPC

页面进入时 store 立即调用 `proxy_status`，可见期间约 4 秒轮询，隐藏窗口跳过轮询；离开页面停止定时器。动作链如下：

```text
Proxy store -> api.ts -> invoke(command)
  proxy_status   -> core.alive(/version), /proxies, /configs, registry.status, SQLite fallback
  select_proxy   -> PUT /proxies/{group} -> best-effort SQLite remembered selection
  test_delay     -> /proxies -> collect names -> concurrent /delay
  set_system_proxy -> alive check -> SystemProxyManager enable/disable
  switch_rule_mode -> PATCH /configs -> remembered mode
  recover_system_proxy -> SystemProxyManager.recover
```

`set_system_proxy(true)` 在 sidecar 不存活时拒绝执行，避免把系统流量指向死端口。`SystemProxyManager` 的 `enable`、`disable`、`startup_check` 共用操作锁，防止并发开启把开启态误保存为旧现场。注册表及 SQLite 同步 IO 必须在命令层的 blocking 线程执行，不能在 setup runtime 内直接调用。

## 事实源与生命周期

- mihomo 存活：`GET /version`。
- 代理组、当前节点和延迟历史：`GET /proxies`；切换后由 mihomo 保持运行态。
- 规则模式：`GET/PATCH /configs`；SQLite `KEY_RULE_MODE` 只作不可达时的展示兜底。
- 系统代理：Windows Internet Settings 注册表；`ProxyBackupState` 只记录 Toogl 开启前现场。
- 运行端口：sidecar 本次运行内存值；`KEY_CONTROLLER_PORT`、`KEY_MIXED_PORT` 供定时任务和下一阶段解析使用。

退出时 `lib.rs` 先调用 `cleanup_system_proxy_on_exit`，若备份存在且注册表仍开启则恢复旧现场，再调用 sidecar `shutdown` 设置主动回收标记并 kill 子进程。异常退出无法执行钩子时，下次首个 `proxy_status`/开关调用触发一次 `startup_check`：仍开启则把残留提示交给前端确认，已关闭则清理失效备份。sidecar 输出必须持续 drain，否则子进程可能因管道缓冲满而阻塞。

## 失败安全边界

sidecar 缺失、秒退、端口冲突或就绪超时只使代理模块降级，不阻塞其他模块；错误包含截断的进程输出。用户手写且无模板标记的配置不得自动补线、改 controller 端口或补 secret。模板端口改写不是原子写，若后续需要修改必须先设计配置回滚并补测试。

系统代理开启前保存完整 `ProxyEnable`/`ProxyServer`；关闭或恢复有备份时精确还原并清备份，无备份时只关闭开关而保留用户既有 server。注册表写失败会保留备份，便于下次自检；通知失败会向上报错，不能把通知失败当成成功。退出恢复失败只记录日志，下次启动自检兜底。

前端动作是乐观显示，但动作锁防止同组或同类并发；失败回滚且保留错误。刷新请求和动作共享序号，动作开始后的旧轮询响应只完成等待，不覆盖动作后的事实。后端 `proxy_status` 各字段独立降级，不能把降级字段解释为 mihomo 已应用。

## 修改入口

修改 API 路径、响应转换或并发测速时进入 `crates/core/src/mihomo/client.rs` 并同步模型测试；修改系统代理语义只进入 `crates/core/src/sysproxy.rs`，用注册表 mock 验证；修改 sidecar 端口、secret、配置模板或退出顺序进入 `apps/desktop/src-tauri/src/sidecar.rs`，并检查 `commands/proxy.rs` 的 State 生命周期；修改展示轮询和乐观行为进入 frontend store，并补 store 测试。不要把代理业务写入 `lib.rs`、preferences 或 models 之外的重复 DTO。

## 验证命令与人工步骤

维护者按改动运行定向测试：

```powershell
cargo test -p toogl-core sysproxy
pnpm --filter toogl-desktop test src/modules/proxy/store.spec.ts
```

Windows 人工验收：首次启动确认 config 模板、controller/mixed 端口和 secret；占用默认端口后确认模板顺延；开启系统代理访问普通网页，再关闭确认原有代理值精确恢复；强制结束进程后重启确认残留提示和恢复；用用户手写配置确认不会被自动改写；让 sidecar 秒退或删除二进制确认模块降级且其他页面可用。验收后检查 `toogl-nodes.yaml`、`config.yaml` 与注册表没有遗留测试现场。
