# Repository Guidelines

## Project Overview

Toogl（图格）是面向 Windows 10/11 的个人桌面实用工具箱，采用 Tauri 2 + Vue 3 + TypeScript + Rust。

产品以 mihomo **魔法工具** 为核心，同时提供首页状态看板和软件设置；信息订阅、AI 网关、系统优化、网络优化、开发环境为规划中占位入口（侧栏"规划中"分组，占位页无业务逻辑）。看板展示各模块状态并提供入口，不复制模块业务逻辑。

当前代理只使用 Windows 系统代理，不做 TUN。新增模块采用编译时登记，避免提前引入运行时插件、远程 Hub 或空业务抽象。

## 文档链路

按需阅读：产品边界 → docs/01；架构与实现事实 → docs/02；进度与验收 → docs/03；接入规则 → docs/04。每个功能模块目录有 `README.md` 记录本模块逻辑链路；本文件只放全局约束，与模块 README 不重复。

| 文档                                        | 主要边界                                         |
| ------------------------------------------- | ------------------------------------------------ |
| [产品说明](docs/01-产品说明.md)             | 各模块用户能力、安全边界、明确不做               |
| [架构与数据流](docs/02-架构与数据流.md)     | 目录职责、IPC 清单、启动退出、订阅链、持久化     |
| [开发状态与方向](docs/03-开发状态与方向.md) | 实现进度、待人工验收清单、已知缺口、开发方向     |
| [模块开发指南](docs/04-模块开发指南.md)     | 新增模块步骤、API/Store 规则、测试要求、完成标准 |

文档记录当前代码事实，不把目标方案写成已完成；代码与文档冲突时以代码为准。修改功能时同步所属模块 README 与相关 docs；移动或删除文件时同步引用路径。未实际运行的桌面行为必须注明待验收。

## Architecture & Data Flow

- **调用链**：Vue 页面 → 模块 Pinia Store / `api.ts` → Tauri `invoke` → `apps/desktop/src-tauri/src/commands/` → `crates/core` → SQLite、Windows API、mihomo 或固定外部 API。
- **模块边界**：`apps/desktop/src/app/modules.ts` 是业务导航、业务路由与可选看板卡的唯一登记点；看板与设置固定路由在 `app/router.ts`。模块拥有自己的页面、API、状态和可选摘要卡；看板不复制业务公式。软件设置只管理全局偏好，不依赖业务模块。
- **魔法工具链**：`sidecar.rs` 管理 mihomo 生命周期，`mihomo/client.rs` 调 REST API；`sysproxy.rs` 在开启前备份系统代理，关闭/退出时恢复。退出必须先恢复系统代理，再结束 sidecar。
- **订阅链**：拉取/粘贴 → 解析 → 与手动节点合并（同名手动优先）→ 托管文件 → mihomo 热重载。更新互斥，热重载失败回滚节点文件；24 小时后台更新由 Rust 管理，不依赖页面打开。
- **系统能力边界**：系统能力（进程、注册表、网络修复）使用固定可执行文件和参数，修改类动作必须先展示对象并逐次确认；不接受前端传入任意命令、路径或脚本。规划中的系统优化、网络优化、开发环境模块沿用此边界。
- **持久化**：`<app_data_dir>/toogl.db` 存代理数据、订阅节点元信息和偏好；`PreferenceStore` 使用类型化 namespace JSON。主题 localStorage 仅作首屏缓存；控制密钥存放在内核配置和代理设置中。
- **跨语言类型**：Rust `models/` 是单一来源，ts-rs 生成 `@toogl/core-types`。`packages/core-types/bindings/`（含 `index.ts`）禁止手改。

- **组件与封装**：`App.vue` 只装配布局；侧栏组件位于 `app/components/`。各模块有独立页面、`api.ts`、局部 `use*.ts` 和展示面板。`components/ui/` 仅基础 UI，`shared/` 为跨模块交互，`lib/` 为纯函数。不要复制实现或添加空服务层。
- **实现进度**：已实现魔法工具、看板壳与设置；信息订阅、AI 网关、系统优化、网络优化、开发环境已登记为"规划中"占位入口（`modules/planned/PlannedPlaceholder.vue`，不发起业务 IPC），进度与验收见 `docs/03-开发状态与方向.md`，不能写成已实现或已验收。各模块主目录有 README 辅助功能理解。

## Key Directories

| 路径                                                 | 修改职责                                             |
| ---------------------------------------------------- | ---------------------------------------------------- |
| `apps/desktop/src/app/`                              | 模块登记、路由与应用壳导航组件                       |
| `apps/desktop/src/modules/dashboard/`                | 首页状态看板布局，不持有业务公式                     |
| `apps/desktop/src/modules/planned/`                  | 五个规划中模块的共用占位页                           |
| `apps/desktop/src/modules/proxy/`                    | 魔法工具：代理、节点、订阅和导入                     |
| `apps/desktop/src/modules/settings/`                 | Toogl 主题和版本信息                                 |
| `apps/desktop/src/shared/`、`components/ui/`、`lib/` | 通用错误、UI 和格式化能力                            |
| `apps/desktop/src-tauri/src/`                        | 桌面生命周期、commands、mihomo sidecar 管理          |
| `crates/core/src/`                                   | mihomo、subscription、代理设置、系统代理、偏好和模型 |
| `crates/core/migrations/`                            | 只追加的 SQLite schema 迁移                          |
| `packages/core-types/`                               | Rust 生成的前端类型                                  |
| `docs/`                                              | 产品说明、架构数据流、进度方向和模块接入规范         |

## Development Commands

以下命令均在仓库根目录的 PowerShell 中执行；启动类命令仅作参考，执行前遵守下文协作边界。

```powershell
pnpm install                                      # 安装依赖并激活 pre-commit 钩子
pnpm dev                                          # 仅 Vite，固定端口 1420
pnpm tauri dev                                    # 完整桌面应用（前端 + Rust）
pnpm build                                        # vue-tsc 类型检查 + Vite 生产构建
pnpm tauri build                                  # Windows NSIS 安装包，自动构建前端
pnpm test:front                                   # Vitest 单次运行
cargo test --workspace                            # Rust 全量测试并导出类型
cargo test -p toogl-core export_bindings           # 改 Rust models 后生成 TS bindings
pnpm lint                                         # ESLint；自动修复用 pnpm lint:fix
pnpm format:check                                 # Prettier；格式化用 pnpm format
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

聚焦测试示例：

```powershell
pnpm --filter toogl-desktop test src/modules/proxy/store.spec.ts
cargo test -p toogl-core subscription
```

## Code Conventions & Common Patterns

- **语言与命名**：对话、注释、文档和提交说明使用中文；提交格式保留 Conventional Commits 的类型和小写模块 scope，如 `fix(proxy): 恢复原有系统代理设置`。仅在用户明确授权后执行提交。沿用 `modules/<id>/api.ts`、`store.ts`、可选 `components/OverviewCard.vue` 的组织方式；小工具无需空层。
- **Vue / TS**：使用 `<script setup>`、Composition API、Pinia；页面和较重库按需加载。TS strict，启用 `noUnusedLocals` / `noUnusedParameters`；有意未使用的标识符用 `_` 前缀。`@/` 指向 `apps/desktop/src/`，类型使用 `import type`。
- **格式**：Prettier 无分号、单引号、100 列、尾逗号、LF；Rust 用 rustfmt、4 空格。ESLint 管规则；`.gitattributes` 强制文本 LF。
- **IPC 与错误**：前端 `api.ts` 与命令一一对应，参数/返回类型来自 `@toogl/core-types`。顶层 invoke 参数使用 camelCase，DTO 字段遵循生成类型的 serde 命名。commands 统一返回 `Result<T, CommandError>`；前端用 `shared/errors.ts` 的 `toCommandError` 规整。Rust 内部错误用 thiserror；`CommandErrorKind` 使用生成枚举，不另写字符串集合。
- **异步与状态**：初始化请求复用；后台刷新保留可用缓存；防止旧响应覆盖新状态。页面轮询成对启动/停止，跳过隐藏窗口与在途请求；乐观更新失败回滚。持续后台任务由 `tauri::async_runtime::spawn` 装配，核心异步使用 Tokio。
- **依赖注入**：沿用显式 trait 接缝，例如系统代理 `RegistryIo` / `ProxyStateStore`；测试注入内存实现，不为统一形式增加服务容器或事件总线。
- **偏好**：通过 `PreferenceStore` 的类型化读写扩展配置，不向前端开放任意键写入。读取失败不能当作首次使用并写回默认值；旧值仅在迁移成功后删除。
- **数据库**：新增 `crates/core/migrations/NNNN_description.sql` 并追加 `db.rs::MIGRATIONS`，版本递增、保留历史。每个迁移及版本登记在同一事务中；语句需幂等。执行器按分号切分，SQL 字符串字面量内禁止分号。
- **系统能力边界**：系统能力（进程、注册表、网络修复）使用固定可执行文件和参数，修改类动作必须先展示对象并逐次确认；不接受前端传入任意命令、路径或脚本。
- **UI**：Tailwind CSS 4 + shadcn-vue（new-york / neutral）+ lucide。不引入 Naive UI、Element Plus 或 UnoCSS。

## Important Files

- `apps/desktop/src/main.ts`：Vue、Pinia、路由与主题初始化；`app/router.ts` 使用 hash 路由。
- `apps/desktop/src/app/modules.ts`：全部模块入口登记（魔法工具已实现；五个规划中模块以 `planned` 占位）；实现规划模块时替换组件、删 `planned`、补看板卡，步骤见 `docs/04-模块开发指南.md`。
- `apps/desktop/src-tauri/src/main.rs`：WebView2 启动探测；`lib.rs`：状态装配、命令登记、后台任务和退出清理。
- `apps/desktop/src-tauri/src/sidecar.rs`：端口探测、进程管理、`MIHOMO_EXPECTED_VERSION`。
- `crates/core/src/models/mod.rs`、`models/error.rs`：模型导出入口和统一错误契约。
- `crates/core/src/db.rs`、`preferences.rs`、`proxy_setting.rs`：迁移、普通偏好和代理设置。
- `apps/desktop/src-tauri/tauri.conf.json`、`apps/desktop/vite.config.ts`、`apps/desktop/vitest.config.ts`、`apps/desktop/tsconfig.app.json`：构建、CSP、测试和类型规则。
- `package.json`、`pnpm-workspace.yaml`、`Cargo.toml`：工作区与命令；`.github/workflows/ci.yml`：Windows CI 质量门禁。
- `docs/01-产品说明.md`、`docs/02-架构与数据流.md`、`docs/03-开发状态与方向.md`：产品范围、架构事实和人工验收；代码与文档冲突时以代码为准，文档不得把计划功能标为完成。

## Runtime/Tooling Preferences

- 使用 Node.js 24 LTS、`pnpm@12.4.1` 与 Rust stable。保留 pnpm/Cargo workspace 和 lockfile，不切换 Bun、npm 或 Yarn。
- 仅支持 Windows 10/11 + WebView2。不要扩展 macOS/Linux 桌面适配。Vite `strictPort` 使用 1420；浏览器中的 `pnpm dev` 无法独立验证 Tauri IPC 与系统能力。
- mihomo 使用仓库内固定 Windows sidecar（当前 v1.19.32）；替换二进制时同步 `sidecar.rs::MIHOMO_EXPECTED_VERSION`。controller 只监听回环并保留 secret 鉴权。
- 保留 Tauri CSP；改动后验证开发窗口及生产包资源加载。pre-commit 仅对暂存的前端文件运行 Prettier / ESLint，全仓检查由 CI 执行。

## AI 助手协作边界

- **语言**：对话始终使用中文；新增或修改的代码注释、文档和说明也使用中文。代码中的 API、包名、命令、文件名、协议字段和已有英文标识必须保留原始拼写。
- **终端**：当前开发环境是 Windows 11，终端是 Windows Terminal 中的 PowerShell。命令必须兼容实际 PowerShell 版本，不假定支持 `&&` 等版本相关语法；不要直接使用 Bash heredoc、`export`、`source` 等语法。文件路径含空格时正确引用，文本读写使用 UTF-8 并保留仓库 LF 约定。优先使用专用工具读取、搜索和编辑文件。
- **文件权限**：在当前项目目录及子目录内，为完成任务可以直接新增、修改和删除文件，无需逐次确认；此授权不包含绕过 Git 限制或操作项目外的文件。修改前先读取相关内容，不擅自覆盖用户已有改动，不顺手清理无关文件或全仓格式化。
- **Git 边界**：允许只读查看 `git status`、`git diff`、`git log`、`git show`。其他写入暂存区、历史、分支或远端的操作必须先获得用户明确同意，包括 `add`、`commit`、`restore`、`reset`、`checkout`、`switch`、`clean`、`stash`、`merge`、`rebase`、`pull`、`push` 等。
- **项目启动**：默认用户已启动项目，不自行运行 `pnpm dev`、`pnpm tauri dev`、预览服务器或其他长期服务。确需启动、停止或重启项目时，先说明原因并获得用户同意；不得擅自终止用户进程、清理端口占用或换端口重复启动。
- **浏览器验证**：默认不使用浏览器打开页面、截图或自动化交互；只有用户明确要求时才执行。UI、真实 Tauri IPC 和桌面行为由用户在现有运行环境中手动验证，助手提供具体操作、预期结果和需要反馈的信息。可以自行运行不启动项目的类型检查、lint、单元测试和构建；不得借测试修改真实凭据、代理配置或用户数据。
- **变更与反馈**：先定位受影响的调用方、类型和测试，再编辑；完成后优先运行与改动直接相关且不启动项目的检查。若需要真实环境才能确认，明确区分已验证事实与待用户验证项，不用推测结果替代验证。

## Testing & QA

- **前端**：Vitest + jsdom，测试邻近源码命名 `*.spec.ts`；重点是 Pinia 状态转换、看板卡片加载隔离、偏好迁移、轮询/失败回滚、错误处理与 IPC 契约。组件测试从简，避免脆弱快照。
- **Rust**：源码内 `#[cfg(test)]` 单元测试；HTTP 使用 wiremock，数据库使用隔离 SQLite；注册表使用内存 mock，不修改真实系统现场。覆盖订阅解析与更新、代理状态、持久化及恢复。
- **类型检查**：`models/mod.rs` 的导出测试生成当前类型并更新 bindings；改 models 后生成结果保留在工作区供审阅，不自动执行 Git 提交。CI 在 Rust 测试后执行 `git diff --exit-code -- packages/core-types/bindings` 防止漂移。
- **门禁**：Windows CI 执行 ESLint、Prettier、Vitest、rustfmt、Clippy（`-D warnings`）、Rust 全量测试与 bindings 一致性检查，最后构建 Tauri 安装包。当前未配置覆盖率数值门槛或 e2e 套件。
- **真实能力验收**：涉及 sidecar、系统代理、订阅、系统能力修改、CSP 的变更需由用户在真实桌面验证启动/退出、恢复、失败保留旧数据和安全边界。助手按 `docs/03-开发状态与方向.md` 的待人工验收清单提供操作说明，不擅自启动项目或浏览器。
