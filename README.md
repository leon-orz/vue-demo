# workspace-root

全新的空 workspace 骨架，等待新需求开发。

- pnpm workspace：`pnpm-workspace.yaml`（当前 `packages/*` 暂无成员）
- Cargo workspace：`Cargo.toml`（当前 `members = []` 暂无成员）
- 工程配置：ESLint（flat config）、Prettier、EditorConfig、git hooks 待按需恢复

## 常用命令

```powershell
pnpm install        # 安装依赖
pnpm lint           # ESLint
pnpm format:check   # Prettier 检查
```
