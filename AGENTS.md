# AGENTS.md

> 本文件面向 AI 编码助手，描述 Toogl（图格）项目的架构、命令与约定。
> 项目权威文档在 `docs/`：`01-需求文档.md`（PRD，模块 M1~M6）、`02-开发计划.md`（技术选型、里程碑、规范与测试）、`03-部署文档.md`（运维 playbook）。代码与文档冲突时以代码为准，术语与阶段划分以文档为准。

## 项目概述

Toogl（图格）是一个**个人自用的 Windows 桌面工具箱**：Tauri 2 桌面壳 + Vue 3 前端 + Rust 后端，monorepo（pnpm workspace + Cargo workspace）。**仅支持 Windows**（NF-14），不处理 macOS/Linux 桌面端。

主要功能模块（编号与 PRD 对齐）：

- **M1 AI 用量统计（当前仅 Kimi Code）**：解析 kimi-code `~/.kimi-code/sessions/**/wire.jsonl` 本地 CLI 日志，聚合 token 用量 / 缓存命中率 / 趋势 / 分布；配额余额走 Kimi 适配器（开放平台 `/v1/users/me/balance` 余额 + 非官方 `/coding/v1/usages` best-effort）。只读聚合，不转发流量。**2026-10-02 决策：MiMo Code / omp / DeepSeek 三个平台的 parser、适配器与 UI 已全部砍掉**（代码在 git 历史中，omp/dsh/MiMo 将来回归时从 git 捞）；schema 与命令层的 platform 维度保持泛化，migration 0006 清理存量非 kimi 数据。不支持 Codex CLI 日志；GLM 套餐不列入；dsh 为阶段三预留。
- **M2 我的 API 平台（已取消）**：不再自建 new-api 网关，需求并入 M1，编号保留不复用。取消原因：官方套餐直连已够用、MiMo 条款禁止套餐额度用于非编程场景、免去网关运维负担。
- **M3 服务器 / M4 NAS（已取消 2026-10-02）**：原为一体机的监控、容器管理、WoL 远程开机、磁盘健康等（阶段二起）。取消原因：用户近期无时间维护一体机；编号保留不复用。
- **M5 魔法工具**：内嵌 mihomo sidecar（RESTful API 默认 9090），做节点切换、系统代理开关、规则分流。目标场景为软件与网站（不考虑游戏）；节点导入以订阅链接为主（用户当前为商家包月订阅）、手动粘贴为辅（后续 VPS 自建）。
- **M6 设置 + Dashboard**。

分阶段开发：阶段一（不依赖新硬件，桌面端直连模式）→ 阶段二（一体机到位，部署 hub 中枢服务，不再部署 new-api；**已冻结 2026-10-02**：M3/M4 取消，hub 骨架保留冻结）→ 阶段三（按需增强）。代码中的 `TODO(S1-2)` 等标记对应各阶段任务号。

## 仓库结构

```
apps/
  desktop/                # 桌面客户端 toogl-desktop（Tauri 2 + Vue 3）
    src/                  # Vue 前端：api/（invoke 封装，与 Rust 命令一一对应）、
                          #   stores/（Pinia）、views/、components/、lib/
    src-tauri/            # Rust 壳：src/commands/（Tauri commands 薄壳）、
                          #   sidecar.rs（mihomo 进程管理，已落地）、binaries/（sidecar 二进制）
  hub/                    # 中枢服务 toogl-hub（axum + tokio，跑在一体机 Linux 上，无界面，阶段二已冻结）
crates/
  core/                   # 共享核心 toogl-core：models / adapters（kimi 配额适配器）/
                          #   datasource / keystore / mihomo / kimicode（wire.jsonl 日志 parser）/
                          #   logs/（文件游标扫描入库骨架，按平台泛化保留）/
                          #   usage/（编排层：query 聚合查询、quota 配额刷新、account 账户配置）/
                          #   sysproxy.rs（系统代理读写与现场备份还原）、proxy_setting.rs（代理设置持久化）、
                          #   db.rs（SQLite 连接与迁移执行）
                          #   migrations/（SQLite schema，由 src/db.rs 按版本号迁移执行）
                          #   （mimocode / omp parser 与 deepseek / mimo 适配器已随 2026-10-02 决策删除，git 历史可恢复）
packages/
  core-types/             # @toogl/core-types：TS 类型，ts-rs 从 Rust models 生成
  ui/                     # @toogl/ui：共享 Vue 组件（预留，基本为空）
docs/                     # 需求 / 开发计划 / 部署三份中文文档
.github/workflows/ci.yml  # CI
```

关键架构原则：

- **Rust 核心下沉**：数据模型、平台适配器、日志解析、加解密全部在 `crates/core`；桌面端 Tauri commands 和 hub HTTP 接口都是它的薄壳，不写重复业务逻辑。
- **数据获取层可替换**：`DataSource` trait（`crates/core/src/datasource/`），阶段一只有 `DirectSource`（直连），阶段二加 `HubSource`（经中枢），UI 层零改动。
- **类型单一来源**：TS 类型由 ts-rs 从 `crates/core/src/models` 的 Rust 结构体生成到 `packages/core-types/bindings/`（`#[ts(export)]` + `export_bindings` 测试）。

## 环境与常用命令

环境要求：Node.js 24 LTS、pnpm 12（corepack/Volta 管理，根 `package.json` 的 `packageManager` 锁定）、Rust stable（rustup）、Windows 10/11 + WebView2。

```bash
pnpm install                # 安装依赖（同时激活 pre-commit 钩子）
pnpm dev                    # Vite dev server（端口 1420，strictPort）
pnpm tauri dev              # 起桌面壳应用（前端 + Rust 一起）
pnpm build                  # 前端生产构建：vue-tsc -b 类型检查 + vite build
pnpm tauri build            # 打 Windows NSIS 安装包
pnpm lint / pnpm lint:fix   # ESLint 9 flat config
pnpm format / format:check  # Prettier
cargo test --workspace      # Rust 全部测试（含 ts-rs 类型导出测试）
cargo fmt / cargo clippy --workspace --all-targets -- -D warnings
cargo test -p toogl-core export_bindings   # 改 models 后重新生成 TS bindings
cargo test -p toogl-core -- --ignored keyring_real_roundtrip   # 真实系统钥匙串冒烟测试（手动）
```

## 代码风格与约定

- **注释与文档用中文**；提交信息用英文 Conventional Commits，scope 用模块小写名（usage/server/nas/proxy/settings/hub/core）。
- **Rust**：rustfmt（4 空格缩进，见 `.editorconfig`）+ clippy，CI 中 `-D warnings`。错误类型用 thiserror；内部 trait 允许 `async fn`（已 `#[allow(async_fn_in_trait)]`）。
- **TS/Vue**：ESLint 管规则、Prettier 管格式，职责分开（`eslint-config-prettier` 收尾）。Prettier：无分号、单引号、printWidth 100、trailing comma all、LF。TS strict 全开 + `noUnusedLocals` / `noUnusedParameters`；有意未使用的变量用下划线前缀（ESLint 约定）。Vue 用 `<script setup>` + Composition API；页面级组件允许单词命名。
- **前端路径别名**：`@/` → `apps/desktop/src/`（vite.config.ts 与 tsconfig 均配置）。
- **类型纪律（重要）**：`packages/core-types/bindings/` **禁止手改**，只由 ts-rs 生成；前端 `invoke` 的参数/返回类型必须来自该包。改 Rust models 后跑 `export_bindings` 测试并提交生成物，CI 用 `git diff --exit-code` 校验。
- **Tauri commands**：`src-tauri/src/commands/` 只做薄壳调 `toogl_core`，前端在 `src/api/` 写一一对应的 invoke 封装。所有命令统一返回 `Result<T, CommandError>`（`crates/core/src/models/error.rs`，ts-rs 导出）；前端 invoke 抛出的即该 JSON 对象，经 `src/api/errors.ts` 的 `toCommandError` 规整后展示/分支。
- **SQLite migration**：schema 统一收归 Rust 侧（tauri-plugin-sql 已移除，前端不直连数据库）。在 `crates/core/migrations/` 新增 `NNNN_description.sql`，并在 `crates/core/src/db.rs` 的 `MIGRATIONS` 追加，`version` 严格递增、只增不改；SQL 按分号切分逐条执行（**禁止字符串字面量内含分号**），全部语句幂等（IF NOT EXISTS）。
- **UI 组件**：shadcn-vue（new-york 风格、neutral 底色、lucide 图标，见 `components.json`）+ Tailwind CSS 4；**不使用成品 UI 库**（Naive UI / Element Plus 已排除），不混用 UnoCSS。图表用 ECharts 按需引入（控制包体积）。
- **提交钩子**：simple-git-hooks + lint-staged，只对暂存文件跑 prettier + eslint --fix；全仓检查由 CI 兜底。
- **依赖管理**：精确版本 + lockfile 提交；crates.io 走 rsproxy 国内镜像。

## 测试策略

- **Rust（主力）**：`cargo test --workspace`。日志解析器（M1 心脏）用脱敏 wire.jsonl fixture 驱动单元测试，覆盖增量偏移与聚合边界；HTTP 适配器用 wiremock mock server 测错误路径（401/超时/字段缺失）。`#[ignore]` 标注依赖真实系统能力的测试（如 keyring 冒烟），手动执行。
- **前端**：Vitest 测关键 store（usage/proxy/settings），组件测试从简，不写脆弱快照测试。
- **e2e**：tauri-driver + WebdriverIO，只覆盖 3 条核心链路（启动见数据 / 切换节点 / 系统代理开关）。
- **CI**（`.github/workflows/ci.yml`，windows-latest）：`pnpm lint` + `format:check` + `pnpm test:front`（Vitest）；`cargo fmt --check` + `clippy -D warnings` + `cargo test` + bindings 一致性校验；最后 `tauri build`。
- 每里程碑按《02-开发计划》第 4 节验收标准过真实环境。

## 安全注意事项

- **敏感 key 存系统钥匙串**：桌面端用 keyring-rs 走 Windows 凭据管理器。**坑：keyring 3 默认不启用平台后端（静默退化为 mock 内存存储），必须显式 `features = ["windows-native"]`**。hub（Linux 无桌面）用同一 `SecretStore` trait，底层退回主密钥加密的文件存储；测试用 `MemoryStore`。
- 所有设备经 Tailscale 私网互联，hub 不开公网端口（生产监听 Tailscale 网卡，开发期绑回环 `127.0.0.1:7130`）。
- 系统代理退出残留是体验事故：M5 需启动自检 + 退出钩子双重保障。
- `tauri.conf.json` 当前 `csp: null`，收紧 CSP 时注意兼容性。
- mihomo sidecar 只打 Windows 二进制，选官方 release 固定版本。

## 当前状态

阶段一 S1-3 / S1-4 已完成（脚手架 S1-1 + M1 用量统计 + M5 魔法工具核心三件套）；**2026-10-02 起 M1 收缩为只统计 Kimi Code**：

- 已就绪：monorepo 脚手架与 CI、凭据读写收进 `platform_account_set`（keyring 显式 windows-native，SERVICE_NAME 统一在 `keystore::KEYRING_SERVICE_NAME`）、Rust 侧统一 schema 迁移（`crates/core/migrations/`，事件/日聚合/游标表带 `platform`+`cli` 维度，另有 quota_snapshot / platform_account / proxy_setting，由 `db::migrate` 幂等执行；tauri-plugin-sql 已移除）、kimicode wire.jsonl parser（归一化为 UsageEvent，走 `crates/core/logs/` 共享增量游标 + 按日预聚合骨架；单文件容错 + size/mtime 变化触发幂等重扫；扫描互斥锁防并发双计）、`crates/core/usage/` 编排层（聚合查询 query / 配额刷新 quota / 账户配置 account）、Kimi 配额适配器（`adapters/kimi.rs` 余额 + 非官方 coding usages best-effort；wiremock 覆盖 401/超时/字段缺失）、Tauri commands（`commands/usage.rs` 九条命令，统一返回结构化 `CommandError`）、前端 Kimi 单平台用量页（`views/Usage.vue` + QuotaCard 降级态；`stores/usage.ts`）、Dashboard Kimi 摘要卡 + 代理卡、设置页 Kimi 分区（key + api_base + 日志目录覆盖）、ts-rs bindings 链路（含 index.ts 完整性测试）。Platform 枚举收缩为仅 `Kimi`（PlatformFilter 保留 all/kimi），schema 与命令层 platform 维度泛化不动，migration 0006 清理存量非 kimi 数据。
- M5 魔法工具（S1-3）：mihomo v1.19.32 sidecar（`src-tauri/binaries/` 固定版本，sha256 与官方 release digest 一致；externalBin + tauri-plugin-shell；setup 拉起 / `RunEvent::Exit` 回收；external-controller 仅回环、端口顺延 9090-9100 持久化到 proxy_setting；首跑生成仅回环初始配置模板）、`crates/core/mihomo/client.rs` RESTful 客户端（代理组/切节点/并发测速/模式切换/存活探测）、`crates/core/sysproxy.rs` 系统代理（RegistryIo/ProxyStateStore seam，开启备份现场/关闭无损还原/崩溃残留自检恢复，测试全 mock 不碰真实注册表）、proxy 命令六条（`commands/proxy.rs`：`proxy_status` 字段级降级、`select_proxy`、`test_delay`、`set_system_proxy`、`switch_rule_mode`、`recover_system_proxy`；退出钩子先恢复系统代理再杀 sidecar）、前端 `views/Proxy.vue` + `stores/proxy.ts`（4s 轮询、乐观更新失败回滚、残留提示恢复流）。
- 骨架待落地：节点导入（M5-07 分享链接/订阅，S1-3 拆出的后续任务）、HubSource（S2-2）、hub API /usage /config /control（S2-1，现仅 /api/v1/health）。（S2 项随阶段二冻结暂停）
- **2026-10-02 决策**：M3 服务器 / M4 NAS 两模块取消（用户近期无时间维护一体机，编号保留不复用），阶段二（一体机/hub）随之冻结；`apps/hub` 骨架保留冻结，仅 `/api/v1/health`。同日 M1 收缩：MiMo Code / omp / DeepSeek 的 parser（`mimocode/`、`omp/`）、适配器（`adapters/mimo.rs`、`deepseek.rs`）与前端多平台 UI 全部删除（git 历史可恢复），omp/dsh/MiMo 将来回归时从 git 捞。
- 已知占位：用量估值单价表全 0（estimated_cost 恒 0，待官方价格确认，将迁移为 M6 可配置项）。
- 人工验收待补：Kimi `/coding/v1/usages` 格式为推测待真机确认（S1-4 其余两项随平台砍掉作废）；S1-3 真机联调（sidecar 拉起无孤儿进程、切节点出口 IP 变化、系统代理注册表实测、强杀后残留自检恢复）。
