# Toogl（图格）

面向 Windows 10/11 的个人桌面实用工具箱。以 **mihomo 魔法工具** 为核心，辅以首页状态看板和软件设置；信息订阅、AI 网关、系统优化、网络优化、开发环境为规划中模块（侧栏"规划中"分组，占位页无业务逻辑）。

技术栈：Tauri 2、Vue 3、TypeScript、Pinia、Rust、SQLite、mihomo；pnpm 与 Cargo workspace。当前只使用 Windows 系统代理，不做 TUN、远程 Hub、手机端、多用户。

## 产品结构

```text
首页看板
├─ 魔法工具（已实现）  mihomo、代理节点、模式、系统代理、订阅和手动导入
├─ 信息订阅（规划中）  RSS 与 GitHub 信息聚合
├─ AI 网关（规划中）   AI 模型聚合中转平台用量与状态
├─ 系统优化（规划中）  游戏设置检查与后台占用清理
├─ 网络优化（规划中）  网络状态检测与修复
├─ 开发环境（规划中）  端口冲突、残留进程处理
└─ 软件设置（已实现）  Toogl 主题和版本信息
```

各模块能力边界与"明确不做"见 [产品说明](docs/01-产品说明.md)。

## 环境要求

- Node.js 24 LTS（版本要求见根 `package.json`）
- pnpm 12.4.1（根 `package.json` 的 `packageManager`）
- Rust stable（通过 rustup 安装）
- Windows 10/11 + WebView2

## 常用命令

以下命令在仓库根目录执行：

```powershell
pnpm install            # 安装依赖并激活项目钩子
pnpm dev                # 启动 Vite（固定 :1420）
pnpm tauri dev          # 启动完整 Tauri 桌面应用
pnpm build              # 前端类型检查与生产构建
pnpm lint               # ESLint
pnpm format:check       # Prettier 检查
pnpm test:front         # 前端 Vitest 单次测试
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace  # Rust 测试并导出当前 bindings
pnpm tauri build        # 构建 Windows NSIS 安装包
```

改动 Rust models 后，使用：

```powershell
cargo test -p toogl-core export_bindings
```

生成的 `packages/core-types/bindings/` 禁止手改。

## 目录

- `apps/desktop/src/app`：模块登记、路由及应用壳导航组件
- `apps/desktop/src/modules/dashboard`：首页状态看板布局
- `apps/desktop/src/modules/planned`：五个规划中模块的共用占位页
- `apps/desktop/src/modules/proxy`：魔法工具，管理 mihomo、节点、系统代理和订阅
- `apps/desktop/src/modules/settings`：应用主题和版本信息
- `apps/desktop/src/shared`：错误规整与小型公共能力
- `apps/desktop/src-tauri/src`：Tauri 生命周期、命令层和 sidecar 管理
- `crates/core`：Rust 核心业务、订阅解析、持久化和模型
- `packages/core-types`：由 Rust `ts-rs` 自动生成的 TypeScript 类型
- `docs`：产品说明、架构数据流、进度方向和模块接入规范

各功能目录内有 `README.md`，记录该模块的逻辑链路、关键不变量与文件职责。

## 文档

- [产品说明](docs/01-产品说明.md)：产品定位、模块能力边界、明确不做
- [架构与数据流](docs/02-架构与数据流.md)：目录职责、IPC 清单、启动退出、订阅链、持久化
- [开发状态与方向](docs/03-开发状态与方向.md)：实现进度、待人工验收清单、已知缺口
- [模块开发指南](docs/04-模块开发指南.md)：模块登记、边界、测试和完成标准
- [AGENTS.md](AGENTS.md)：全局约束、协作边界与质量门禁

代码与文档冲突时，以当前代码和实际验收结果为准；计划功能不得写成已完成。
