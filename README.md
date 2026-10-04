# Toogl（图格）

个人桌面工具箱：Tauri 2 + Vue 3 + TypeScript，仅支持 Windows。
monorepo：pnpm workspace + Cargo workspace，详见 `docs/02-开发计划.md`。

## 环境要求

- Node.js 24 LTS（pnpm 由 corepack / Volta 管理，版本锁定见根 `package.json` 的 `packageManager`）
- Rust stable（rustup 安装）
- Windows 10/11（WebView2）

## 常用命令

```bash
pnpm install            # 安装依赖（同时激活 pre-commit 钩子）
pnpm dev                # Vite dev server（:1420）
pnpm tauri dev          # 起桌面壳应用
pnpm build              # 前端生产构建（vue-tsc 类型检查 + vite build）
pnpm lint / pnpm format # ESLint / Prettier
cargo test --workspace  # Rust 测试（含 ts-rs 类型导出）
```

## 目录

- `apps/desktop`：桌面客户端（Tauri 2 + Vue 3 + Pinia + Tailwind CSS + shadcn-vue）
- `apps/hub`：中枢服务（Rust axum，阶段二，已冻结）
- `crates/core`：共享 Rust crate（models / adapters / datasource / keystore / mihomo / subscription 等，详见 `AGENTS.md`）
- `packages/core-types`：TS 类型（ts-rs 生成，禁止手改）
- `packages/ui`：共享 Vue 组件（预留）
