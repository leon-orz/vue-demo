# Repository Guidelines

> 本文档为仓库重建期的指导文件。仓库当前处于空置状态，以下内容基于历史信息（git 历史、历史 package.json / Cargo.toml）与通用工程约定编写，供后续新项目落地时遵循。

## 1. 运行环境与协作硬性约束（最高优先级）

本节为用户明确规定的硬性规则，优先级高于本文档其余内容。

### 1.1 系统与终端环境

- 操作系统：**Windows 11**
- 终端：**PowerShell**
- CLI 辅助工具：**oh-my-pi cli**
- 路径分隔符为 `\`，但工具与配置文件中建议统一使用正斜杠 `/`（Node/pnpm/git 均兼容）

### 1.2 语言约定（中文优先）

- 所有对话、文档（含 README、docs/）、代码注释、提交信息（commit message）**一律以中文为主**。
- 代码标识符（变量、函数、类型）仍用英文命名；仅说明性文字用中文。
- 示例提交信息：`feat(proxy): 新增订阅导入功能`。

### 1.3 命令执行纪律（重要教训）

- 所有命令必须符合当前 Windows/PowerShell 环境，**不得**套用 Linux/macOS 习惯想当然。
- **禁止反复重试**：同一命令因环境原因（权限、路径、编码）失败 2 次后，必须停止重试，改换方案（如改用 PowerShell 原生命令、写 `.ps1` 脚本执行）或向用户说明，不得"一直尝试到成功为止"。
- 复杂或批量文件操作（如删除 `node_modules`、`target`）**优先写 `.ps1` 脚本执行**，避免内联转义问题。已验证的有效模式：
  1. 遍历文件重置属性为 `Normal`（pnpm 安装的包可能带只读属性）；
  2. `Remove-Item -Recurse -Force`。
- 编码统一 UTF-8：`.ps1` 脚本、配置文件、源码均用 UTF-8（无 BOM 或按工具约定），避免终端中文乱码。
- 注意 PowerShell 版本差异：`&&` 链接符在 Windows PowerShell 5.1 中不可用；跨版本兼容的脚本避免依赖它。
- 项目内脚本尽量收敛为 `pnpm` scripts（`package.json`），由 pnpm 负责跨平台执行，不在文档/说明里裸写 shell 命令。

### 1.4 Git 操作权限（硬性约束）

**允许自主执行：**

- 查看类：`git status`、`git log`、`git diff`、`git show`、`git branch`（只读查看）
- 提交类：`git add` + `git commit`（提交信息遵循第 7 节约定）

**禁止自主执行（必须先征得用户同意）：**

- 一切改写历史/回退操作：`reset`、`revert`、`rebase`、`amend`、`filter-branch` 等
- 丢弃工作区/暂存区内容：`checkout -- <file>`、`restore`、`clean`、`stash`（含 pop/drop）
- 远端操作：`push`、`pull`、`fetch`、`clone` 其他仓库、远端分支管理
- 分支/tag 的创建、切换、删除、合并等结构操作
- `--force`、`--hard` 等破坏性参数

> 用户明确指示的操作视为已获同意；指示不明确时先询问再执行。

## 2. Project Overview

- 本仓库（`vue-demo`）目标形态是一个 **前端为主、可含 Rust 辅助组件** 的 monorepo。
- 历史配置显示：
  - `package.json` 曾使用 **pnpm@12.4.1** 作为包管理器，要求 **Node >= 24**；
  - `Cargo.toml` 曾是 **Cargo workspace**（`resolver = "2"`，`edition = "2021"`）。
- 新项目落地时，应延续上述技术栈基线（pnpm + Node 24+ + Cargo workspace），除非另有明确决策。

## 3. Repository Structure

- **当前状态：仓库已被彻底清空，仅保留 `.git` 目录，不存在任何源码、测试或配置文件。** git 历史可提供参考：
  - `cffe654`：旧业务代码（旧工具模块、占位 UI、dashboard 看板等）；
  - `c290d15`：workspace 骨架（pnpm / Cargo workspace 配置）；
  - `cd20b85`：清空全部文件，仅保留 git 历史。
- 新项目落地后的预期结构（约定，待建立）：
  ```
  /                  # 根：package.json、pnpm-workspace.yaml、Cargo.toml(workspace)
  ├── apps/          # 应用类子包
  ├── packages/      # 共享库类子包
  ├── crates/        # Rust workspace 成员 crate
  ├── docs/          # 文档
  └── tests/ 或各包内联测试
  ```

## 4. Setup & Prerequisites

- Node.js >= 24（以历史 package.json 的 engines 约束为准）。
- 包管理器：pnpm（曾锁定 pnpm@12.4.1，建议使用 `corepack` 或 `packageManager` 字段固定版本）。
- Rust 工具链：若涉及 workspace，使用 edition 2021、resolver 2。
- **待新项目落地后补充**：具体安装与初始化步骤（如 `pnpm install`、`cargo fetch` 等）。

## 5. Development Commands

- **待新项目落地后补充**。当前仓库无 package.json / Cargo.toml，无法提供可执行命令。
- 落地后应在此节登记（示例占位，不代表已存在）：
  - 安装依赖 / 构建 / 开发服务器 / lint / format / 测试等命令及其所属 workspace 成员。
- 所有命令按 1.3 节要求以 PowerShell 兼容方式书写与执行。

## 6. Coding Conventions

以下为可持续执行的通用约束：

- **Workspace 优先**：新增功能先考虑放入已有的共享包/ crate，避免在各 app 内复制实现；跨包引用一律走 workspace 依赖声明，不硬编码相对路径或重复安装。
- **命名**：文件与目录用 kebab-case，TypeScript/JavaScript 符号遵循各自惯用法（PascalCase 组件、camelCase 变量），Rust 遵循 rustfmt 默认约定。
- **注释语言**：代码注释用中文（见 1.2 节），简明说明"为什么"而非复述代码。
- **配置单一来源**：版本、lint、format 等配置尽量收敛到仓库根部，子包不重复维护。
- 代码风格细节（缩进、引号、导入排序等）以落地后的统一 formatter 配置为准，不在此处逐条硬编码。

## 7. Commit & PR Guidelines

- **提交信息用中文**，沿用现有 git log 风格：`chore:`、`feat:`、`docs:` 等类型前缀；涉及具体模块时可带 scope，如 `feat(proxy): ...`。主题行简洁、说明"做了什么"。
- 提交粒度：一个提交对应一个完整、可回退的变更；大变更拆分提交。
- 提交即视为获得授权的 git 操作，可直接执行（见 1.4 节）；但**不得**为绕过失败而擅自 `reset` 或改写已提交内容。
- PR/合并约定：待协作流程明确后补充；在此之前，直接向主分支提交也应遵循上述提交信息格式。

## 8. Testing & QA

- **当前仓库无任何测试或 CI 配置**；本节仅约定目标基线。
- 新项目落地后：
  - 每个共享包/ crate 应具备至少基本单元测试；
  - 测试命令纳入根级脚本统一调度（workspace 优先原则）；
  - 提交前应通过 lint + format + test（具体命令待补充）。

## 9. Notes for Future Contributors

- 本文档描述的部分结构、命令与工具链为**基于历史信息的规划**，以仓库中实际存在的配置文件为准；发现不一致时，以实际配置为事实并更新本文档。
- 第 1 节为用户明确规定的硬性约束，新增条目须经用户确认。
- 修改根级 workspace 配置（package.json、Cargo.toml、pnpm-workspace.yaml）属于影响全体成员的变更，应在提交信息中明确说明。
