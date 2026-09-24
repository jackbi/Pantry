# Repository Guidelines

## 项目结构

本仓库是基于 Tauri 2 + Vue 3 + TypeScript 的桌面应用。

- `src/`：Vue 前端源码，入口 `src/main.ts`，根组件 `src/App.vue`，图片放 `src/assets/`
- `src-tauri/`：Rust 后端，命令与 `run()` 在 `src-tauri/src/lib.rs`，配置在 `tauri.conf.json`，权限在 `capabilities/default.json`
- `public/`：原样拷贝的静态资源；`index.html` 为前端入口

## 构建与开发命令

统一使用 pnpm（仓库只有 `pnpm-lock.yaml`）。

```bash
pnpm install        # 安装依赖
pnpm dev            # 启动 Vite 开发服务器（固定端口 1420）
pnpm build          # vue-tsc 类型检查 + vite build，产物在 dist/
pnpm tauri dev      # 启动桌面应用（自动拉起 pnpm dev）
pnpm tauri build    # 打包桌面安装包
```

## 代码风格与命名约定

- 统一 2 空格缩进；TypeScript 使用双引号与分号
- Vue 组件使用 `<script setup lang="ts">` 组合式 API
- 组件文件用 PascalCase（`App.vue`），组合式函数用 `useXxx`，Rust 命令用 snake_case 并标注 `#[tauri::command]`
- `tsconfig.json` 开启 `strict`、`noUnusedLocals`、`noUnusedParameters`，残留未使用变量会导致 `pnpm build` 失败
- 暂无 ESLint/Prettier 配置，格式化跟随编辑器默认值，提交前运行 `pnpm build`

## 测试指南

目前没有测试框架与测试目录，PR 前的最低验证标准：

```bash
pnpm build        # 类型检查通过
pnpm tauri dev    # 手动冒烟：验证 greet 命令与页面渲染
```

若引入测试，请使用 Vitest，文件命名 `*.spec.ts`，与源码同目录或置于 `src/**/__tests__/`。

## 提交与 PR 规范

采用中文 Conventional Commits：

```
<type>[scope]: <summary>
```

- `type` 取 `feat`/`fix`/`docs`/`style`/`refactor`/`perf`/`test`/`build`/`ci`/`chore`/`revert` 等
- `summary` 动词开头、不超过 50 字、不加句号；破坏性变更在 type 后加 `!` 并在 body 说明迁移方式
- 一次提交只做一类变更，body 写清动机而非复述 diff

PR 需包含变更动机、影响范围、关联 issue，UI 变更附截图，并确认 `pnpm build` 通过。

## 安全与配置提示

- 前端通过 `invoke("greet", { name })` 调用 Rust 命令；新增命令需注册到 `lib.rs` 的 `invoke_handler`
- 新增插件或权限必须在 `capabilities/default.json` 声明，否则运行时被拒绝
- 不要提交 `node_modules`、`dist`、`src-tauri/target`、`.codegraph`、`.agents`

## Agent 专用说明

本仓库已建 CodeGraph 索引：查找或理解代码时优先用 `codegraph explore "<符号名或问题>"`，不要先做全文搜索。
