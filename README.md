# Toogl（图格）

Toogl 是面向 Windows 10/11 的个人桌面实用工具箱。

产品以 **mihomo 魔法工具** 为核心，同时保留首页状态看板、网络诊断、系统状态、空间维护和软件设置。首页看板汇总各模块状态并提供入口，不复制模块业务逻辑。

技术栈：Tauri 2、Vue 3、TypeScript、Pinia、Rust、SQLite、mihomo；使用 pnpm 与 Cargo workspace。

## 产品结构

```text
首页看板
├─ 魔法工具       mihomo、代理节点、模式、系统代理、订阅和手动导入
├─ 网络诊断       固定 DNS/TCP 检查和 DNS 缓存刷新
├─ 系统状态       Windows 与设备只读信息
├─ 空间维护       当前用户 Temp 目录安全扫描
└─ 软件设置       Toogl 主题和版本信息
```

魔法工具优先保证真实 Windows 环境下的启动、节点切换、系统代理、退出恢复和订阅可靠性。当前不做 TUN、远程 Hub、手机端、多用户、AI API 网关、游戏加速或全面系统优化。

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

- `apps/desktop/src/app`：模块登记、路由与应用壳组件；`AppSidebar` 负责分组，`SidebarLink` 统一链接样式
- `apps/desktop/src/modules/dashboard`：首页状态看板布局
- `apps/desktop/src/modules/proxy`：魔法工具，管理 mihomo、节点、系统代理和订阅
- `apps/desktop/src/modules/network-diagnostics`：固定网络诊断和 DNS 刷新
- `apps/desktop/src/modules/system-status`：Windows 与设备只读信息
- `apps/desktop/src/modules/storage`：Temp 目录扫描和空间概况
- `apps/desktop/src/modules/settings`：应用主题和版本信息
- `apps/desktop/src/shared`：错误规整、模块标签和小型公共能力
- `apps/desktop/src-tauri/src`：Tauri 生命周期、命令层和 sidecar 管理
- `crates/core`：Rust 核心业务、系统能力、订阅解析、持久化和模型
- `packages/core-types`：由 Rust `ts-rs` 自动生成的 TypeScript 类型
- `docs`：产品需求、开发计划和模块接入指南

### 当前实现与开发边界

- 六个一级入口已有实际页面；三个基础工具不再共用系统工具聚合页。
- 魔法工具已有看板摘要卡；其他模块的看板卡仍按开发计划推进，不能视为已完成。
- 页面负责装配和加载，模块内 `components/` 负责展示，`use*.ts` 管理局部状态，`api.ts` 对接 IPC。
- `system-status/useSystemInformation.ts` 同时供系统状态和空间维护读取系统盘数据，不复制查询逻辑。
- `components/ui/` 保持基础 UI，`shared/` 放跨模块交互，`lib/` 放无副作用纯函数；不添加空服务层。

## 文档

- [产品需求文档](docs/01-需求文档.md)：产品定位、模块范围、行为和验收
- [开发计划](docs/02-开发计划.md)：架构、数据链路、分阶段开发和质量门禁
- [模块开发指南](docs/03-模块开发指南.md)：模块登记、边界、测试和完成标准

代码与文档冲突时，以当前代码和实际验收结果为准；计划功能不得写成已完成。
