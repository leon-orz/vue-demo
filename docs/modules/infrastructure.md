# 公共基础设施维护指南

[返回 AGENTS 索引](../../AGENTS.md) · [模块接入规范](../03-模块开发指南.md) · [设置](settings.md)

## 公共代码职责

| 位置 | 职责与约束 |
| --- | --- |
| `apps/desktop/src/components/ui/` | shadcn-vue 基础组件，不引用业务 Store 和路由 |
| `apps/desktop/src/shared/errors.ts` | 把 Tauri reject、Error、未知值规整成生成的 CommandError |
| `shared/components/LoadErrorBanner.vue` | 只展示错误和触发重试；业务配置入口由调用方提供 |
| `shared/useModuleTab.ts` | 魔法工具 main/config 查询标签，保留其他 query，replace 不堆叠历史 |
| `lib/format.ts`、`lib/formatBytes.ts` | 时间、延迟和字节等无副作用展示函数 |
| `lib/utils.ts` | Tailwind class 合并工具 |
| `crates/core/src/models/` | DTO、错误类型、serde 与 ts-rs 契约 |
| `crates/core/src/db.rs` | 应用 SQLite schema 迁移 |
| `preferences.rs`、`proxy_setting.rs` | 分别管理普通偏好和代理业务持久化 |

`shared/useModuleTab.ts` 当前只有魔法工具使用；名称通用不等于需推广给所有模块。不要增加服务容器、事件总线或通用 repository 来包裹单个调用。

## IPC 与类型生成

API 使用生成类型，顶层 invoke 参数 camelCase，DTO 字段跟随 Rust serde。commands 返回 `Result<T, CommandError>`；`shared/errors.ts` 为未知 kind 使用 internal 兜底，不泄露任意对象结构给组件。

`models/mod.rs` 公开模型，`export_bindings_index` 显式导出模型并生成排序后的包入口；`packages/core-types/bindings/` 禁止手改。新增类型要加入导出列表，不仅加 `#[ts(export)]`。`NodeFailure` 的原始定义属于订阅解析，入口同时负责导出。

```powershell
cargo test -p toogl-core export_bindings
```

生成后审阅工作区差异，不自动提交。CI 在测试后比较生成物，防止代码与类型漂移。

## 数据库事实

路径为 `<app_data_dir>/toogl.db`，使用 WAL 和 busy timeout。代理设置与普通偏好使用各自连接池访问同一个数据库；不是两个互不相干的数据库。

`db.rs` 当前登记版本 5、8、9。旧业务 SQL 文件留作历史，不再执行废弃业务建表；版本 9 移除退役表。迁移器对每条已登记迁移仍重复执行幂等 SQL，以修复登记与真实 schema 不一致；版本行只首次插入。因此“有版本号就跳过 SQL”不是当前行为。

每条迁移与版本登记在独立 `BEGIN IMMEDIATE` 事务执行；SQL 按分号拆分，禁止在字面量和注释中放会破坏拆分的分号。追加迁移必须幂等，不修改历史文件或重用版本号。不能将一次性数据破坏操作伪装为可重复执行迁移。

PreferenceStore 固定 app namespace，缺失返回 `None`、损坏 JSON 返回错误，不能当作缺失。代理 KV 的系统备份和订阅节点见 [魔法工具](proxy.md) 与 [节点来源](subscription.md)。

## 维护与验证

- 公共组件只接受必要 props/emits，不推断业务路由。
- 修改格式函数须考虑 bigint 字节、无效日期、未测速和超时等边界。
- 修改 CommandErrorKind 时同步规整逻辑和错误测试，不能手改生成类型。
- 修改 schema 时先检查迁移重复执行和旧数据兼容，再用隔离 SQLite 验证。

```powershell
pnpm --filter toogl-desktop test src/shared/errors.spec.ts src/lib/format.spec.ts src/lib/formatBytes.spec.ts
cargo test -p toogl-core preferences
cargo test --workspace
```

测试使用隔离数据库和注册表内存实现，不访问真实凭据、代理配置或系统文件。生产包 sidecar、CSP、WebView2 仍需人工验收，不由纯函数/单元测试证明。
