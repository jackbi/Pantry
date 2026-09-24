# Pantry

macOS 桌面版包管理器 GUI：把 npm 生态（npm / pnpm / bun / deno）的全局包与 Homebrew 的 formula / cask 收进一个界面，查看、搜索、安装。

> **Pantry**（储藏室）：本机全局命令行工具与 Homebrew 包的可视化管理。名字取的正是这个意思——货架上有什么、什么版本、占多大，一眼看清；要新的就去 npm 市场或 brew 里取。

- **技术栈**：Tauri 2 + Vue 3（`<script setup lang="ts">`）+ TypeScript + Vite 8 + Tailwind CSS 4
- **状态**：功能已可用，尚未打包分发（路线图阶段 7 进行中）；**没有在本机真实安装过任何包**，首次真实安装由使用者自己点确认
- **界面规范**：[`design-system/pantry/MASTER.md`](design-system/pantry/MASTER.md)

## 能力

| 页面 | 内容 |
|------|------|
| Node 包 | npm / pnpm / bun / deno 的全局包：来源筛选、体积、详情、升级 / 卸载；另一栏是 npm 市场搜索，选一个包管理器安装 |
| Homebrew | 已装 formula / cask 的描述、依赖、caveats、安装量，cask 可「打开」「在访达中显示」，支持安装 / 升级 / 卸载。**只在 macOS 且能解析到 brew 时出现** |
| 诊断 | 包管理器在不在、什么版本、有没有新版本、可执行文件在哪，可升级项带「更新」按钮；PATH 解析、代理探测、命令试跑折在「环境详情」里 |
| 设置 | 包数据源（registry）、代理、跳过 TLS 证书校验、主题 |

安装类操作一律"先摆出将执行的命令、再执行"：输出流式、可随时取消、破坏性操作二次确认，同一时刻只允许一个任务。

## 环境要求

- macOS（Homebrew 那部分只在 macOS 上出现，其余是 Tauri 的通用能力）
- Node.js + **pnpm**（仓库只有 `pnpm-lock.yaml`）
- Rust **原生 arm64 工具链**（Intel 工具链会直接报 `Bad CPU type in executable`）
- 可选：本机装了 npm / pnpm / bun / deno / brew 才有东西可管，缺哪个诊断页会直说

本机验证过的组合：Node v22.23.2 · pnpm 11.25.0 · rustc 1.98.1（aarch64-apple-darwin）。

## 快速开始

```bash
pnpm install

pnpm tauri:dev   # 启动桌面应用（自动拉起前端 dev server，端口固定 1420）
pnpm dev         # 只跑前端：浏览器里调样式，Tauri 命令不可用
pnpm build       # vue-tsc 类型检查 + vite build —— 提交前必须通过
pnpm tauri build # 打包桌面安装包（尚未在本机验证过产物）
```

后端：

```bash
cd src-tauri
cargo test                          # 单测：不联网，也不会安装 / 卸载任何东西
cargo test -- --ignored --nocapture # 真机测试：会真的调用本机命令（只读，例如 brew search / info）
cargo build                         # 编译检查，应无 warning
```

不启动窗口自检环境，用来排查"为什么找不到 npm"：

```bash
./src-tauri/target/debug/hengran-public-store --diagnose
```

输出登录 shell 解析到的 PATH、各包管理器落在哪、代理来自哪里。

## 目录结构

| 路径 | 说明 |
|------|------|
| `src/views/` | 页面：Node 包 / Homebrew / 诊断 / 设置，以及各页内的「已安装」「安装新包」视图 |
| `src/components/ui/` | 设计系统组件（按钮、输入、表格、面板、开关……），样式只写语义类名 |
| `src/composables/` | 主题、弹层滚动锁、已安装列表缓存、设置 |
| `src/lib/` | 与 Tauri 通信、格式化、文案判断等小工具 |
| `src-tauri/src/runner.rs` | 子进程：流式输出、进程组取消、环境变量注入 |
| `src-tauri/src/shell_env.rs` | 登录 shell 的 PATH 解析与缓存 |
| `src-tauri/src/packages.rs` | 已安装包采集（npm / pnpm / bun / deno / brew）与体积测量 |
| `src-tauri/src/market.rs` | npm registry：搜索、详情、版本历史、下载量、README |
| `src-tauri/src/brew.rs` | Homebrew：搜索、详情、安装量、可用性 |
| `src-tauri/src/managers.rs` | 包管理器自检与版本比较 |
| `src-tauri/src/actions.rs` | 安装 / 卸载 / 升级的命令构造与执行 |

## 网络与本机约束

下面几条是第一次跑几乎一定会遇到的，结论都来自本机实测（细节见 `docs/ROADMAP.md`）：

- 默认 registry 用 **npmmirror**：npm 官方源在本机直连不通，必须"走代理 + 跳过证书校验"才通。
- **从访达启动的应用拿不到终端里的 PATH 与代理**，所以应用自己解析登录 shell，并在设置页提供显式填写；这些设置只影响本应用发起的请求和它启动的子进程。
- **不改写**你的 `.npmrc` / brew 配置；不读取 `~/.npmrc` 里的凭据，也不会写进日志。
- 下载量统计与 README 走独立域名（`api.npmjs.org`、`cdn.jsdelivr.net`），换镜像不会改变这两个口径。

## 排障

| 现象 | 原因与处理 |
|------|-----------|
| `cargo metadata ... Bad CPU type in executable` | Rust 工具链架构不对，换成原生 arm64 工具链 |
| `Port 1420 is already in use` | 已经有一个 dev server 在跑（例如另一个 `pnpm tauri:dev`），先关掉再启动 |
| 应用里显示"未找到 npm / brew" | GUI 继承不到终端 PATH：看诊断页「环境详情」，或对照 `--diagnose` 的输出 |
| 官方源搜索超时 | 在设置页填代理并打开「跳过 TLS 证书校验」，或直接用内置默认源 |
| 第一次进「已安装」要等几秒 | 首次采集要跑 `npm ls -g`、`du` 量体积；之后先渲染缓存再后台更新（右上角显示"更新中"） |

## 文档

| 文件 | 用途 |
|------|------|
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | 规格与进度、实测数据与结论、架构取舍、已知陷阱 |
| [`design-system/pantry/MASTER.md`](design-system/pantry/MASTER.md) | 设计规范：色板、排版、组件职责、反模式与交付前检查清单 |
| [`AGENTS.md`](AGENTS.md) | 协作规范：提交信息格式、语言策略、CodeGraph 用法 |

日常开发在 `develop` 分支，`main` 只接受合并。提交信息用中文 Conventional Commits（`<type>[scope]: <summary>`），一次提交只做一类变更；提交前跑 `pnpm build` 与 `cargo test`。
