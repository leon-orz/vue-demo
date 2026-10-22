# 订阅模块维护指南

相关入口：[AGENTS.md](../../AGENTS.md) · [模块开发指南](../03-模块开发指南.md)

## 当前实现

订阅模块把订阅 URL、分享链接/订阅文本、托管节点文件和 mihomo 热重载串成一条本地链。订阅更新流程是：读取并校验 URL → HTTP 拉取 → 识别 Clash/mihomo YAML、整段 base64 或逐行分享链接 → 逐条解析并记录坏行 → 读取手动节点 → 手动优先按 `name` 合并 → 对本工具模板配置执行一次 proxy-provider 接线 → 备份旧 `toogl-nodes.yaml` → 临时文件原子替换 → `PUT /configs?force=true` → 成功后写 SQLite 元信息。

手动导入不访问订阅 URL：解析输入后，以新导入同名节点覆盖旧手动节点，再与最后一次成功订阅节点合并，走相同的接线、文件原子写、热重载和失败回滚。手动节点优先，订阅同名节点让位。订阅失败保留旧节点和旧托管文件；成功才更新 `KEY_SUBSCRIPTION_NODES`、`KEY_SUBSCRIPTION_LAST_UPDATED` 并清除 `last_error`。

已实现：HTTP/HTTPS 订阅、Clash/mihomo YAML、base64 分享链接、vless/hysteria2/trojan/ss 分享链接、逐条失败明细、手动节点并集、24 小时定时刷新、热重载失败回滚。未实现：任意协议的通用转换服务、远程账户管理、TUN/PAC 和自动替换用户手写配置。

## 文件职责

- `apps/desktop/src/modules/proxy/subscription-api.ts`：订阅 command IPC 一对一封装。
- `apps/desktop/src/modules/proxy/subscription.ts`：订阅状态轮询、防重入、错误保留和导入结果展示。
- `apps/desktop/src-tauri/src/commands/subscription.rs`：State 装配、命令参数、24 小时循环和 sidecar controller client 构造；业务编排在 core。
- `crates/core/src/subscription/fetch.rs`：URL 校验、UA、超时和 HTTP 错误分类。
- `crates/core/src/subscription/content.rs`：内容形态识别、逐行容错和失败明细。
- `crates/core/src/subscription/share.rs`：分享链接到 mihomo `Mapping` 的纯函数解析。
- `crates/core/src/subscription/config.rs`：模板标记判断、provider 接线、托管节点文件原子写/恢复。
- `crates/core/src/subscription/update.rs`：订阅和手动导入的合并、热重载、KV 落盘、失败记录和共享互斥。
- `crates/core/src/proxy_setting.rs`：URL、节点 JSON、更新时间和错误等键；数据事实需按流程解释，不能把节点 KV 当作已加载内核的唯一事实。

## 调用链与 IPC

```text
Proxy.vue subscription UI
  -> subscription.ts store
  -> subscription-api.ts
  -> invoke(command)
     subscription_status -> read_status(SQLite)
     subscription_set_url -> validate URL -> save URL -> run_update
     subscription_update -> run_update
     import_share_text -> import_merge

run_update/import_merge
  -> fetch/parse/merge
  -> ensure_wired(config.yaml)
  -> write_nodes_file(toogl-nodes.yaml, tmp+rename)
  -> MihomoClient.reload_config(force=true)
  -> write node metadata and timestamps to proxy_setting
```

命令与定时任务共享 `Arc<AtomicBool>`。更新入口使用 compare-exchange 抢占，已在更新时直接返回错误，不排队；任务在 await 期间被取消也必须释放标志。`subscription_refresh_loop` 启动时检查 URL 和成功时间，过期或从未成功则补刷，之后每 24 小时执行；sidecar controller 端口未知或 `/version` 不可达时只写 `last_error` 并继续循环。

## 事实源与生命周期

订阅 URL、上次成功时间、错误文案和最后成功订阅节点列表存于 `proxy_setting`。手动节点也存于 `KEY_MANUAL_NODES`。这些 KV 是导入编排的持久化事实，但 mihomo 当前运行的代理组/节点仍以 `/proxies` 为准。托管文件 `app_data_dir/mihomo/toogl-nodes.yaml` 是 provider 的文件事实；`config.yaml` 只在本工具模板上自动接线，用户手写配置要求手工接入。

节点文件更新先写同目录临时文件并 rename，避免内核读到半截内容。热重载成功后才写节点 JSON、更新时间和清错；因此拉取、解析、接线、写文件或热重载失败不会把新节点元信息伪装成已生效。热重载失败会用更新前文件内容原子恢复；没有旧文件则删除新文件。恢复失败会明确写入错误，表示磁盘文件与内核可能暂时不一致，重启后需人工检查/重新加载。

定时任务由 setup 持有独立 `SubscriptionUpdater`，和命令 State 共享同一 SQLite 文件及更新标志。应用退出不会等待网络更新；正在 await 的任务结束后由 RAII guard 清理互斥标志，下一次启动重新从 KV 判断是否过期。

## 失败安全边界

URL 只接受带 host 的 HTTP/HTTPS，HTTP 非 2xx 按状态分类，拉取有固定超时并使用 Clash 客户端 UA。未知或坏节点只记 `NodeFailure`；解析结果没有可用节点时由解析层报错，不能用空列表覆盖旧节点。

接线失败发生在托管文件写入前，文件不动。写入使用原子替换；热重载失败恢复旧文件并保留旧 KV。KV 写入失败发生在内核已重载之后，错误会向上抛并记录 `last_error`，此时必须以 mihomo 和托管文件为准，不能把 KV 写失败误称为完全回滚。

`ensure_wired` 仅识别本工具模板标记并修改模板；用户手写 `config.yaml` 不自动插入 provider 或代理组。节点名称是合并键，同名覆盖属于设计契约；没有 `name` 的条目不会被当作可安全去重的同名项。不要在前端自行合并订阅和手动节点，也不要把 HTTP 订阅内容直接交给 mihomo。

更新互斥是跨命令和定时任务的关键边界。不要改成可排队锁，除非同时重新设计取消、UI 状态和退出行为；不要在 await 外手工复位标志。文件回滚只覆盖托管节点文件，不能把用户手改的 `config.yaml` 作为自动回滚目标。

## 修改入口

修改支持的输入格式进入 `content.rs`/`share.rs`，为每种格式补纯函数测试；修改 URL、超时或 HTTP 错误分类进入 `fetch.rs`；修改接线或原子文件语义进入 `config.rs`；修改合并、回滚、互斥和 KV 提交顺序进入 `update.rs`。修改命令名或状态循环进入 Tauri `commands/subscription.rs`，修改页面状态进入 `subscription.ts`。跨语言字段必须改 Rust models 后重新生成 bindings，不得手改 `packages/core-types/bindings`。

## 验证命令与人工步骤

维护者定向运行 `cargo test -p toogl-core subscription` 与 `pnpm --filter toogl-desktop test src/modules/proxy/subscription.spec.ts`。需要全量检查时遵循仓库 CI 命令。

Windows 人工验收：配置有效 HTTP 订阅并确认节点文件、provider 接线、代理组和 `/proxies` 更新；输入坏 URL、401、500、超时和坏行，确认旧节点保留且错误可见；导入同名手动节点确认手动优先；让 `/configs` 热重载失败确认文件回滚和 KV 未前移；删除或修改用户手写 config 确认不会静默接线；等待或模拟过期时间确认启动补刷，sidecar 不可达时确认循环不退出并记录错误。验收后删除测试订阅和托管文件中的敏感节点内容。
