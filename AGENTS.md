# AGENTS.md

> 本文件面向 AI 编码助手，描述 Toogl（图格）项目的架构、命令与约定。
> 项目权威文档在 `docs/`：`01-需求文档.md`（PRD，模块 M1~M6）、`02-开发计划.md`（技术选型、里程碑、规范与测试）、`03-部署文档.md`（运维 playbook）。代码与文档冲突时以代码为准，术语与阶段划分以文档为准。

## 项目概述

Toogl（图格）是一个**个人自用的 Windows 桌面工具箱**：Tauri 2 桌面壳 + Vue 3 前端 + Rust 后端，monorepo（pnpm workspace + Cargo workspace）。**仅支持 Windows**（NF-14），不处理 macOS/Linux 桌面端。

主要功能模块（编号与 PRD 对齐）：

- **M1 Kimi 统计**：解析本机 `~/.kimi-code/sessions/**/wire.jsonl` 会话日志，聚合 token 用量 / 缓存命中率 / 最近 5 小时窗口；另查 Kimi 开放平台余额。
- **M2 我的 API 平台**：自建 new-api 网关用量只读展示。
- **M3 服务器 / M4 NAS**：一体机的监控、容器管理、WoL 远程开机、磁盘健康（阶段二起）。
- **M5 魔法工具**：内嵌 mihomo sidecar（RESTful API 默认 9090），做节点切换、系统代理开关、规则分流。
- **M6 设置 + Dashboard**。

分阶段开发：阶段一（不依赖新硬件，桌面端直连模式）→ 阶段二（一体机到位，部署 hub 中枢与 new-api）→ 阶段三（按需增强）。代码中的 `TODO(S1-2)` 等标记对应各阶段任务号。

## 仓库结构

```
apps/
  desktop/                # 桌面客户端 toogl-desktop（Tauri 2 + Vue 3）
    src/                  # Vue 前端：api/（invoke 封装，与 Rust 命令一一对应）、
                          #   stores/（Pinia）、views/、components/、lib/
    src-tauri/            # Rust 壳：src/commands/（Tauri commands 薄壳）、
                          #   sidecar.rs（mihomo 进程管理，待落地）、
                          #   migrations/（SQLite 迁移）、binaries/（sidecar 二进制）
  hub/                    # 中枢服务 toogl-hub（axum + tokio，跑在一体机 Linux 上，无界面，阶段二）
crates/
  core/                   # 共享核心 toogl-core：models / adapters / datasource / keystore / mihomo
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

- **注释与文档用中文**；提交信息用英文 Conventional Commits，scope 用模块小写名（kimi/gateway/server/nas/proxy/settings/hub/core）。
- **Rust**：rustfmt（4 空格缩进，见 `.editorconfig`）+ clippy，CI 中 `-D warnings`。错误类型用 thiserror；内部 trait 允许 `async fn`（已 `#[allow(async_fn_in_trait)]`）。
- **TS/Vue**：ESLint 管规则、Prettier 管格式，职责分开（`eslint-config-prettier` 收尾）。Prettier：无分号、单引号、printWidth 100、trailing comma all、LF。TS strict 全开 + `noUnusedLocals` / `noUnusedParameters`；有意未使用的变量用下划线前缀（ESLint 约定）。Vue 用 `<script setup>` + Composition API；页面级组件允许单词命名。
- **前端路径别名**：`@/` → `apps/desktop/src/`（vite.config.ts 与 tsconfig 均配置）。
- **类型纪律（重要）**：`packages/core-types/bindings/` **禁止手改**，只由 ts-rs 生成；前端 `invoke` 的参数/返回类型必须来自该包。改 Rust models 后跑 `export_bindings` 测试并提交生成物，CI 用 `git diff --exit-code` 校验。
- **Tauri commands**：`src-tauri/src/commands/` 只做薄壳调 `toogl_core`，前端在 `src/api/` 写一一对应的 invoke 封装。
- **SQLite migration**：在 `src-tauri/migrations/` 新增 `NNNN_description.sql`，并在 `src-tauri/src/lib.rs` 的 `migrations()` 追加，`version` 严格递增、只增不改。
- **UI 组件**：shadcn-vue（new-york 风格、neutral 底色、lucide 图标，见 `components.json`）+ Tailwind CSS 4；**不使用成品 UI 库**（Naive UI / Element Plus 已排除），不混用 UnoCSS。图表用 ECharts 按需引入（控制包体积）。
- **提交钩子**：simple-git-hooks + lint-staged，只对暂存文件跑 prettier + eslint --fix；全仓检查由 CI 兜底。
- **依赖管理**：精确版本 + lockfile 提交；crates.io 走 rsproxy 国内镜像。

## 测试策略

- **Rust（主力）**：`cargo test --workspace`。日志解析器（M1 心脏）用脱敏 wire.jsonl fixture 驱动单元测试，覆盖增量偏移与聚合边界；HTTP 适配器用 wiremock mock server 测错误路径（401/超时/字段缺失）。`#[ignore]` 标注依赖真实系统能力的测试（如 keyring 冒烟），手动执行。
- **前端**：Vitest 测关键 store（kimi/proxy/settings），组件测试从简，不写脆弱快照测试。
- **e2e**：tauri-driver + WebdriverIO，只覆盖 3 条核心链路（启动见数据 / 切换节点 / 系统代理开关）。
- **CI**（`.github/workflows/ci.yml`，windows-latest）：`pnpm lint` + `format:check`；`cargo fmt --check` + `clippy -D warnings` + `cargo test` + bindings 一致性校验；最后 `tauri build`。
- 每里程碑按《02-开发计划》第 4 节验收标准过真实环境。

## 安全注意事项

- **敏感 key 存系统钥匙串**：桌面端用 keyring-rs 走 Windows 凭据管理器。**坑：keyring 3 默认不启用平台后端（静默退化为 mock 内存存储），必须显式 `features = ["windows-native"]`**。hub（Linux 无桌面）用同一 `SecretStore` trait，底层退回主密钥加密的文件存储；测试用 `MemoryStore`。
- 所有设备经 Tailscale 私网互联，hub 不开公网端口（生产监听 Tailscale 网卡，开发期绑回环 `127.0.0.1:7130`）。
- 系统代理退出残留是体验事故：M5 需启动自检 + 退出钩子双重保障。
- `tauri.conf.json` 当前 `csp: null`，收紧 CSP 时注意兼容性。
- mihomo sidecar 只打 Windows 二进制，选官方 release 固定版本。

## 当前状态

阶段一早期（S1-1 前后）：脚手架、keystore 命令、SQLite migration 框架、`UsageSnapshot`/`Credential` 模型与 ts-rs 链路已就绪；平台适配器、mihomo 客户端、wire.jsonl 解析器、hub API（`/usage` `/config` `/control`）等多为带 TODO 的骨架，任务号见各文件注释与《02-开发计划》。
