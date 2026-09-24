# Repository Guidelines

## 项目结构

本仓库是基于 Tauri 2 + Vue 3 + TypeScript 的桌面应用。

- `src/`：Vue 前端源码。入口 `src/main.ts`，根组件 `src/App.vue`；页面在 `src/views/`，设计系统组件在 `src/components/ui/`，状态与副逻辑在 `src/composables/`，纯函数工具在 `src/lib/`
- `src-tauri/`：Rust 后端。命令与 `run()` 在 `src-tauri/src/lib.rs`，配置在 `tauri.conf.json`，权限在 `capabilities/default.json`；子进程执行、包采集、registry、brew 分别落在 `runner.rs` / `packages.rs` / `market.rs` / `brew.rs`
- `public/`：原样拷贝的静态资源；`index.html` 为前端入口
- `docs/ROADMAP.md`：规格、进度、实测结论与已知陷阱；`design-system/pantry/MASTER.md`：界面规范（颜色、排版、组件职责、反模式）

## 构建与开发命令

统一使用 pnpm（仓库只有 `pnpm-lock.yaml`）。

```bash
pnpm install        # 安装依赖
pnpm dev            # 启动 Vite 开发服务器（固定端口 1420）
pnpm build          # vue-tsc 类型检查 + vite build，产物在 dist/
pnpm tauri:dev      # 启动桌面应用（自动拉起 pnpm dev）
pnpm tauri build    # 打包桌面安装包
```

## 代码风格与命名约定

- 统一 2 空格缩进；TypeScript 使用双引号与分号
- Vue 组件使用 `<script setup lang="ts">` 组合式 API
- 组件文件用 PascalCase（`App.vue`），组合式函数用 `useXxx`，Rust 命令用 snake_case 并标注 `#[tauri::command]`
- `tsconfig.json` 开启 `strict`、`noUnusedLocals`、`noUnusedParameters`，残留未使用变量会导致 `pnpm build` 失败
- 暂无 ESLint/Prettier 配置，格式化跟随编辑器默认值，提交前运行 `pnpm build`

## 测试指南

Rust 侧有完整单测（包名校验、命令构造、版本比较、输出解析等），前端暂时没有测试框架。

```bash
cd src-tauri
cargo test                          # 单测：不联网，也不会安装 / 卸载任何东西
cargo test -- --ignored --nocapture # 真机测试：会真的调用本机命令（只读）
```

PR 前的最低验证标准：`pnpm build` 类型检查通过 + `cargo test` 全绿，改到界面时手动跑一遍 `pnpm tauri:dev`。
若引入前端测试，请使用 Vitest，文件命名 `*.spec.ts`，与源码同目录或置于 `src/**/__tests__/`。

## 提交与 PR 规范

采用中文 Conventional Commits：

```
<type>[scope]: <summary>
```

- `type` 取 `feat`/`fix`/`docs`/`style`/`refactor`/`perf`/`test`/`build`/`ci`/`chore`/`revert` 等
- `summary` 动词开头、不超过 50 字、不加句号；破坏性变更在 type 后加 `!` 并在 body 说明迁移方式
- 一次提交只做一类变更，body 写清动机而非复述 diff

PR 需包含变更动机、影响范围、关联 issue，UI 变更附截图，并确认 `pnpm build` 通过。

## 分支策略

- `main`：可交付状态，只接受合并，不直接在上面开发
- `develop`：日常开发与优化的集成分支，默认在这里提交
- 多步完成的工作从 `develop` 拉 `feat/<主题>` / `fix/<主题>` 分支，做完合回 `develop`
- 发布时把 `develop` 合进 `main`，并在 `main` 上打 tag

## 安全与配置提示

- 前端通过 `invoke("<命令名>", payload)` 调用 Rust 命令（例如 `invoke("list_installed_packages", { sources })`）；新增命令必须注册到 `lib.rs` 的 `invoke_handler`
- 新增插件或权限必须在 `capabilities/default.json` 声明，否则运行时被拒绝
- 不要提交 `node_modules`、`dist`、`src-tauri/target`、`.codegraph`、`.agents`、`.playwright-mcp`
- 不读取 `~/.npmrc` 里的凭据，也不把任何 token 写进日志或界面

## Agent 专用说明

本仓库已建 CodeGraph 索引：查找或理解代码时优先用 `codegraph explore "<符号名或问题>"`，不要先做全文搜索。

改界面前先读 `design-system/pantry/MASTER.md`（那里列了反模式与交付前检查清单）；新功能或取舍写进 `docs/ROADMAP.md`，不要只留在对话里。
