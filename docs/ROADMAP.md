# Pantry 开发路线图

桌面版包管理器：统一查看、搜索、安装 npm 生态（npm、pnpm、bun、deno）的全局包与 Homebrew 的包。

技术栈：Tauri 2 + Vue 3 + TypeScript + Vite 8；CSS 使用 Tailwind CSS 4；UI 设计遵循 `ui-ux-pro-max` 技能；同时支持明亮与黑暗两套主题。

## 0. 环境约束（已实测）

以下都是已在本机验证过的事实，直接影响架构：

- **PATH**：npm/pnpm/node 在 `~/.vite-plus/bin`，bun 在 `~/.bun/bin`，brew 在 `/opt/homebrew/bin`。Finder 启动的 GUI 应用继承不到登录 shell 的 PATH，必须先用 `zsh -lic 'echo $PATH'` 解析并缓存，再 spawn 子进程。
- **网络**：`registry.npmjs.org` 直连不通（`SSL_ERROR_SYSCALL`）。命令行工具能工作，是因为环境里有 `HTTP_PROXY/HTTPS_PROXY=127.0.0.1:7890` 与 `NODE_TLS_REJECT_UNAUTHORIZED=0`；Rust 侧 HTTP 客户端默认两者都没有。`registry.npmmirror.com` 直连可用（搜索约 1.9s、详情约 0.15s），作为默认源。
- **网络（下载量接口另算）**：`api.npmjs.org` 与 registry **不是同一台机器**，直连稳定 0.8s，而套上同一个本地代理会让它挂起（同一个 URL 实测 17.5s 后失败）。所以下载量客户端显式忽略 env/系统代理，只在用户显式配置代理时才走代理。
- **Rust**：已切到原生 arm64 工具链（1.98.1），crates 源走 rsproxy 镜像（配置在 `~/.cargo/config.toml`）。
- **brew 调用成本**：每条 brew 命令都是一次 Ruby 启动，约 0.35s；能合并就合并（`brew info --json=v2` 支持一次问多个包），并行比串行省一半以上。`formulae.brew.sh` 直连要 7.5s、走本机代理 0.87s。
- **本地数据成本**：`npm ls -g` 2.6s（pnpm 0.22s、brew 0.76s），而 `du -sk` 量 46 个 npm 全局包要 7.2s、110 个 brew 包 0.61s——瓶颈是文件数带来的 stat。`du` 分 4 路并行后 3.3s、8 路 2.8s，取 4 路。
- **deno**：本机未安装，且 Deno 没有列出全局包的命令，只能扫 `$DENO_INSTALL_ROOT/bin`，必须优雅降级。
- **安全**：`~/.npmrc` 含明文 auth token，应用不要读取，更不要输出到日志或界面。

## 信息架构

导航按**生态**分，每个生态页内部再分「已安装 / 安装新包」：

| 页面 | 内容 |
|------|------|
| Node 包 | npm / pnpm / bun / deno 的全局包 + npm 市场搜索与安装 |
| Homebrew | 已装的 formula / cask + brew 搜索与安装；**只在 macOS 且能找到 brew 时出现** |
| 诊断 / 设置 | 环境自检、命令试跑；设置 |

- 原来独立的「已安装」页去掉了：它按来源聚合，把两种生态的操作混在一个列表里，反而要先选来源才能干活
- 采集按页面过滤来源（`list_installed_packages(sources)`）：Node 页不再等 `brew info` 的一两秒，Homebrew 页也不跑 npm 系
- Homebrew 入口由 `brew_availability` 决定（系统 == macOS 且能解析到 brew），命令面板（⌘K）同步过滤，入口消失时自动退回 Node 包页
- 诊断页默认只回答四个问题：**包管理器在不在、什么版本、有没有新版本、可执行文件在哪**（"在哪"直接决定"找不到命令"怎么修，所以留在默认表格里；PATH 列表、代理与连通性、命令试跑才折进「环境详情」，折叠状态下这些面板根本不渲染）
- 版本对比用**比较**而不是"不相等"：本地装了 canary 而 registry 是稳定版时，不相等会误报可升级（`managers::compare_versions`，有单测）。已安装列表也走同一个比较器——前端把 `{name, local, latest}` 交给 `newer_versions` 命令判断，不再自己比。Homebrew 用 `brew update` 更新自己，不参与对比，界面写"brew update 自更新"
- 自检不联网部分并行执行：本机 6 项实测 213ms
- 自检清单含 **node**：npm 与 pnpm 都跑在它上面，看不到 node 版本等于少一半信息。node 只读版本，不给更新按钮（升级走 nvm / Homebrew 等），也不参与版本对比——npm 上的 `node` 是另一个包，版本号不是运行时版本
- **brew 只在 macOS 上探测**（`managers::is_relevant`，有单测），其它系统采集与表格里都没有它
- 可升级项与 brew 都有「更新」按钮：npm `install -g npm@latest`、pnpm `add -g pnpm@latest`、bun / deno 各自的 `upgrade`、brew `update`。命令写死在 Rust 侧（有单测），不做"尽力而为"的猜测
- 更新在弹层里跑：先摆出将执行的命令，再执行，输出流式、可取消，成功后自动重新自检把新版本读回来。若命令路径在 `/opt/homebrew/` 或 `/usr/local/Cellar/` 下，弹层会提示改用 `brew upgrade` 更稳妥

## 进度

| 阶段 | 状态 | 说明 |
|------|------|------|
| 0. 环境约束 | ✅ | 已实测并固化为自动化验证 |
| 1. 设计与主题基建 | ✅ | 设计系统 + Tailwind 4 + 双主题；已按 WailBrew / npmx 完成视觉重构 |
| 2. 技术验证 | ✅ | 四项验证通过，产品决策待你确认 |
| 3. 已安装包扫描 | ✅ | 四路真实数据已接入，本机采到 161 个包 |
| 4. npm 市场 | ✅ | 搜索 / 详情 / 当前标签 / 版本历史 / 下载量与趋势 / README 按需加载已接入 |
| 5. 安装 / 卸载 | ✅ | 命令构造、并发锁、二次确认、提权重试已接入；未在本机真实安装过包 |
| 6. brew GUI 化 | ✅ | 搜索 / 详情 / 安装升级卸载 / cask 打开与在访达中显示 / 更新策略已落地；未真实执行过安装 |
| 7. 设置与打磨 | 🔶 | 数据源与代理可配置并已贯通查询与安装命令；默认包管理器、打包分发待做 |
| 信息架构 | ✅ | 按生态分页：Node 包 / Homebrew，各自「已安装 / 安装新包」；Homebrew 入口条件显示 |

## 1. 设计与主题基建

### 1.1 设计系统（ui-ux-pro-max）

- [x] 生成并持久化设计系统到 `design-system/pantry/MASTER.md`
- [x] 风格基线定为 **Minimalism & Swiss Style**。技能默认产出的是 `dark-mode-oled`，标注 `Light Mode: not-recommended`，与双主题要求冲突，因此弃用并重写
- [x] 字体：IBM Plex Sans + JetBrains Mono。**决策**：不引第三方字体包，写成"优先使用该字体，缺失时回退系统字体"的栈，离线也能正常显示
- [x] 图标统一用 SVG（Lucide / Phosphor），禁止 emoji 充当结构图标
- [x] MASTER.md 中记录了组件规格、反模式与交付前检查清单

生成的模板还包含两处不合格内容，已在新版 MASTER.md 中纠正：`outline: none`（移除焦点指示）、12/24px 的营销页控件尺寸（桌面工具改为 32px 控件、40px 列表行）。

### 1.2 Tailwind CSS 4

- [x] `tailwindcss@4.3.3` + `@tailwindcss/vite@4.3.3` 已装，Vite 插件已注册，无需 PostCSS
- [x] 语义 token 定义在 `src/styles/theme.css`：`@theme inline` 映射 `--color-*`，`@theme` 放字体与圆角，组件只写 `bg-surface`、`text-muted-foreground` 这类语义类名
- [x] 圆角刻度已建立：`rounded-control`（6px）、`rounded-card`（10px）
- [x] 间距按 4/8px 节奏使用；z-index 分层随浮层出现时再落地

### 1.3 明亮 / 黑暗双主题

- [x] 主题策略：跟随系统 + 手动亮/暗切换，`src/composables/useTheme.ts` 实现
- [x] Tailwind class 策略：`@custom-variant dark`，切换 `<html class="dark">`
- [x] 在 `main.ts` 挂载前调用 `initTheme()`，避免首帧白闪；并调用 Tauri 窗口 `setTheme()` 让标题栏跟随
- [x] 暗色为独立色阶取值，不是反相
- [x] 对比度已用脚本逐对计算（结果表在 MASTER.md）：正文 17.85:1 / 13.81:1，次要文字 7.58:1 / 6.64:1，控制边框 3.50:1 / 3.58:1
- [x] 控件四态（hover / focus-visible / active / disabled）：共享组件在 `AppButton` / `AppInput` 里统一实现；侧栏导航、顶栏「跳转」、诊断页折叠按钮这些手写控件补上了 `active:`（焦点环来自全局 `:focus-visible`）
- [ ] 完整界面走查：需等阶段 3 的真实数据界面做完再跑

**待确认的偏离**：主题偏好目前存 `localStorage`（webview 内持久化，重启保留）。`tauri-plugin-store` 留给后续的收藏与安装历史，避免为一个布尔值引入插件与权限配置。

### 1.4 视觉重构（参考 WailBrew / npmx）

- [x] 配色从 slate 改为**中性黑白 + 单一蓝色强调**：主按钮用黑底白字（明亮）/ 白底黑字（黑暗），强调色只用于链接、当前项指示条与焦点环
- [x] 取消阴影与渐变，全部改用 1px 细边框；圆角收小到 4px（控件）/ 6px（卡片）
- [x] 信息架构对齐 WailBrew：分组侧栏、可排序密集表格（名称 / 版本 / 来源 / 体积 / 操作）、筛选片、表格下方详情面板与空态
- [x] 排版对齐 npmx：等宽用于包名/版本/路径/计数/键帽，`⌘K` 命令面板，`/` 前缀搜索框
- [x] **中文适配决策**：JetBrains Mono 无中文字形，因此中文标签一律不用等宽与 uppercase，改用 `label-mini`（11px / 500 / 0.06em）
- [x] 新增组件：`AppTable`、`AppDetailPane`、`AppEmptyState`、`AppSearchField`、`AppTag`、`AppKeycap`、`CommandPalette`
- [x] 修复：删除了阶段 0 遗留在 `App.vue` 里的临时全局 `<style>` 块，它给所有 `button` 加了边框并重新定义了 `--surface` / `--border`，在系统暗色下会覆盖新 token
- [x] 修复：外层滚动条。`sr-only` 的表格标题没有已定位祖先，按初始包含块定位后被算进文档坐标，把视口高度从 520px 撑到 634px；给 `<main>` 加 `relative` 解决
- [x] 修复：诊断页"程序 / 参数"输入框错位 20px。原因是参数列多一行 hint、各列高度不等又用了 `items-end`；改为把说明放在整行下方

## 2. 技术验证

- [x] 流式输出与取消链路：`src-tauri/src/runner.rs`（5 项单测：输出分流、环境变量注入、进程组取消、事件字段契约；全仓共 74 项）
- [x] PATH 解析：`src-tauri/src/shell_env.rs`，登录 shell 解析 + OnceLock 缓存 + 兜底目录
- [x] HTTP 层与代理解析：`src-tauri/src/probe.rs`。实测 npmmirror 直连 184ms；npmjs 必须"走代理 + 跳过证书校验"才通（980ms），系统代理本身是关闭的（`scutil` 里 `HTTPEnable: 0`），代理只来自终端环境变量
- [x] **决策**：自建 `#[tauri::command]` + `std::process::Command`，不用 `tauri-plugin-shell`。理由是要自己控制进程组与输出切分，插件封装挡路
- [x] 新增 `--diagnose` 命令行自检：不启动窗口，直接输出 PATH 解析、管理器路径与代理来源，用于排查"为什么找不到 npm"

### 验证证据

`env -i HOME=$HOME ./target/debug/hengran-public-store --diagnose`（只有 HOME 的干净环境，模拟 Finder 启动）解析结果：

| 管理器 | 解析结果 |
|--------|----------|
| npm | `~/.vite-plus/bin/npm` |
| pnpm | `~/Library/pnpm/bin/pnpm` |
| bun | `~/.bun/bin/bun` |
| brew | `/opt/homebrew/bin/brew` |
| deno | 未找到（按预期降级） |

`source: loginShell`、`fallbacksAdded: []`，兜底目录一个都没用上。

### 待你确认的产品决策

1. **升级 / 卸载**：建议安装与卸载一起做（卸载是安装的对称操作），升级单独一轮迭代，避免第一版就碰批量升级的破坏性操作。
2. **deno**：建议做兼容但降级——检测不到就隐藏入口；能检测到时扫描 `~/.deno/bin` 列已装命令，安装走 `deno install -g npm:<pkg>` 并显式传权限参数。
3. **brew cask 的 sudo**：建议用 `osascript ... with administrator privileges` 触发系统原生密码框；失败则回退为"复制命令到终端手动执行"，不在应用内自己收密码。

## 3. 已安装包扫描

- [x] npm：`npm ls -g --depth=0 --json`，根目录取 `npm root -g`（本机是 `/usr/local/lib/node_modules`，与 npm 二进制位置不同，未写死）
- [x] pnpm：`pnpm ls -g --depth=0 --json`；跳过 pnpm 自己（`@pnpm/exe*`）。**已知未做**：同一个包存在于多个全局版本目录时不做去重，本机只有 1 个 pnpm 全局包，复现不了，等有真实重复环境再定"保留哪一个"
- [x] bun：`bun pm ls -g` 文本树解析，按最后一个 `@` 切分以兼容 `@scope/name@version`
- [x] deno：扫 `~/.deno/bin`；**未安装时返回明确的 SourceError 而不是 0 个包**，否则界面上无法区分"没装"和"装了但没有全局包"
- [x] brew：`brew info --json=v2 --installed` 一次取全量，formula 与 cask 统一标记 `kind`
- [x] 采集并行化（每来源一线程），单来源失败只记录错误、不影响其余来源，前端逐条展示失败原因
- [x] 体积：单独一次 `du -sk` 批量测量，不阻塞首屏；测不到就显示 `—`，不显示错误的 0
- [x] 前端：来源筛选片带真实计数、四列可排序、点包名选中、详情面板展示路径/说明/主页
- [x] 本机实测：npm 46 / pnpm 1 / bun 1 / brew 113 = 161 个包，158 条路径体积全部测出
- [x] 版本落后标记：区分**版本升级**与**仅修订更新**。brew 的 `outdated` 有两种含义——`versions.stable` 与已安装版本不同（真正的升级），或版本号相同但 formula 修订号变了（需按新配方重建）。本机 9 个过期项里 llvm 属于后者，早期版本会错误显示成 `23.1.1 → 23.1.1`
- [x] npm 系（npm / pnpm / bun）比对 registry latest：批量请求 `/latest`，按 chunk 限并发 6、上限 120 个包，走与市场页相同的 10 分钟缓存。本机实测 typescript 6.0.3 → 7.0.2 这类信息现在可见

### 按反馈调整（第三轮）

- [x] **无更新版本时置灰升级**：llvm 那种"仅修订更新"的情况不再提供可点击的「升级到最新」，按钮置灰并说明原因
- [x] 徽标措辞从「可重建」改为中性的「配方已更新」，不再暗示存在可执行的动作
- [x] 卸载改为**模态二次确认**：`role="dialog"` + `aria-modal`，展示将执行的完整命令，初始焦点落在「取消」以防误按回车，Esc 可关闭
- [x] **详情面板从页面底部移到右侧常驻列**（`grid-cols-[minmax(0,1fr)_22rem]` + sticky），不再需要滚到页面底部
- [x] 窗口默认尺寸 800×600 → 1200×800，并设 `minWidth` 960 / `minHeight` 600，否则右侧列放不下
- [x] 修复嵌套滚动：详情列只做 `sticky`，去掉 `max-h + overflow-y-auto`（此前它与主滚动并列成两根滚动条）；README 移到模态中查看，并在弹层打开时用 `useScrollLock` 锁住主滚动容器

## 4. npm 市场

- [x] 搜索：`GET {registry}/-/v1/search?text=&size=&from=`，默认 npmmirror，实测 853ms
- [x] 详情：`GET {registry}/{name}/latest`，实测 3.5KB / 0.14s
- [x] **放弃完整 packument**：实测 react 6.8MB / 13s、typescript 15MB / 27s，取它拿 readme 不可行
- [x] README 改为按需从 jsDelivr 定向拉单个文件（`{cdn}/npm/{name}@{version}/README.md`），实测 1–4KB；并由前端的"加载 README"按钮显式触发
- [x] **修正下载量口径**：原来取 npmmirror 的 `/downloads/*`，那统计的是镜像站自己的流量——实测 react 近一周镜像 291 万、官方 **1.33 亿**，差 45 倍。改为官方 `api.npmjs.org` 优先（本机直连 0.8s，比镜像还快），失败才回退镜像并把来源标到界面上
- [x] 周下载量 + 近 7 天趋势：官方 `downloads/point/last-week`（89B）+ `downloads/range/last-week`（346B）
- [x] 当前标签：`{registry}/-/package/{name}/dist-tags`，237B / 0.11s，随详情一起取
- [x] 版本历史（按需加载）：`{registry}/{name}` 完整 packument 只取 `time` / `dist-tags` / 每版 `deprecated`。reqwest 开 gzip 后实测 react 1.27MB、typescript 1.72MB；vue 593 个版本、@deepseek-ai/dsh 26 个版本
- [x] 各版本近一周下载量：官方 `versions/{name}/last-week`（vue 593 个版本 8.8KB；官方不列出零下载版本，界面按 0 处理）
- [x] 作用域包名的编码陷阱：`urlencode` 为了 registry 保留了原文 `/`，但 `/versions/{pkg}/last-week` 只认单段路径，`@scope/name` 会 404——该处改用转义成 `%2F` 的单段编码（有单测钉住）
- [x] 详情字段对齐 npm 官网：版本、许可证、依赖（含版本区间，超过 8 个折叠）、peer 依赖数、解压体积与文件数、发布时间、主页、仓库、废弃警告
- [x] 搜索防抖 300ms + 请求序号（慢响应不会覆盖新结果）+ 后端 10 分钟缓存 + "加载更多"分页
- [x] 版本历史表：版本 / 标签 / 7 日下载 / 发布时间，默认最近 8 个，"显示全部"展开；"共 N 个版本"在加载前就能看到（搜索响应自带 versions 数组，不额外请求）
- [ ] 虚拟滚动：当前一页 20 条，等真正需要时再做
- [ ] README 目前按纯文本展示：包 README 属于第三方内容，直接渲染 Markdown 需要先解决 XSS，暂不引入 markdown 依赖

## 5. 安装 / 卸载

- [x] 命令映射（`src-tauri/src/actions.rs`，忽略测试会打印真实命令供核对）：

  | 来源 | 安装 | 卸载 | 升级 |
  |------|------|------|------|
  | npm | `npm install -g x@v` | `npm uninstall -g x` | 同安装 |
  | pnpm | `pnpm add -g x@v` | `pnpm remove -g x` | 同安装 |
  | bun | `bun add -g x@v` | `bun remove -g x` | 同安装 |
  | deno | `deno install -g -A x@v` | `deno uninstall -g x` | 同安装 |
  | brew | `brew install [--cask] x` | `brew uninstall [--cask] x` | `brew upgrade [--cask] x` |

- [x] 流式输出复用 `proc://event` + `LogViewer`，可随时取消
- [x] 安装方式选择（市场页列出 npm/pnpm/bun/deno，未安装的置灰并说明原因），执行前展示将执行的完整命令
- [x] 包名白名单校验：npm 名规则（含 `@scope/name`）、brew 名规则、版本号规则；11 组注入样本全部拒绝；正常路径一律传参数数组，不拼 shell 字符串
- [x] 并发锁在 Rust 侧强制（前端禁用按钮只是提示），任务退出经回调释放；启动失败也会立即放锁
- [x] 卸载 / 升级二次确认，危险色与取消按钮视觉分离
- [x] brew cask 提权：走 `osascript ... with administrator privileges`，命令按 shell 转义后整体传入；失败输出命中权限关键词时，界面给出"以管理员权限重试"
- [ ] **尚未真实执行过安装**：验证用的是无害命令 + 真实命令构造，没有往本机全局环境安装任何包。首次真实安装请你手动点一次确认

## 6. brew GUI 化

- [x] 独立页面（侧栏「Homebrew」）：搜索 → 详情 → 安装 / 升级 / 卸载。不与 npm 市场混在一个视图里——brew 是本地命令，没有分页、README、下载趋势
- [x] 搜索：`brew search --formula/--cask <kw>`（实测 0.2–0.6s，纯文本，一行一个名字），再用一次 `brew info --json=v2` 批量补描述与安装状态（5 个包 0.32s）。两种类型并行，合计约 1.3s；类型筛选在**前端**做，点一下即时生效
- [x] 详情：`brew info --json=v2 --formula/--cask` → 版本、许可证、tap、依赖、冲突、caveats、废弃/停用原因、cask 的 `auto_updates`（提示该走应用内更新）
- [x] 安装量（npm 周下载量的对应物）：`formulae.brew.sh/api/formula/<name>.json` 的 `analytics.install["30d"]`，实测 ripgrep 30 天 33283 次。只有 formula 有逐包接口，cask 显示"—"并说明原因
- [x] 该统计单独发一次请求：这台机器直连 7.5s、走代理 0.87s，不能让它挡在详情前面（详情本身 0.3s）
- [x] cask 的「打开」「在访达中显示」：路径直接取自 cask `artifacts` 里的 `target`（本机 cc-switch → `/Applications/CC Switch.app`；纯字体/安装器类 cask 没有 App 产物，按钮自动隐藏）
- [x] 该动作在 Rust 侧校验：只放行绝对路径且必须存在，「直接打开」仅限 `.app`（`open` 会按类型交给系统处理）；一律 argv 传参，不经过 shell
- [x] **更新策略**：所有 brew 调用统一带 `HOMEBREW_NO_AUTO_UPDATE=1`。自动更新一天最多触发一次，但一旦触发就是几十秒白等，界面表现为"点了没反应"；需要新索引时到诊断页跑预设「brew 更新索引」
- [ ] 尚未真实执行过 brew 安装 / 卸载：验证用的是真实命令构造 + 无害命令，没往本机装任何东西。首次真实操作请你手动点一次确认

## 本地数据策略（先保留，再更新）

底层命令（`npm ls -g`、`brew info --json=v2 --installed`）**只有全量接口**，没有增量可言，所以"增量拉取"做不到；能做的是不白等：

- **先保留再更新**：采集结果按 scope 缓存在本机（内存 + localStorage）。进页面时先把上次结果画出来，同时后台重扫——切视图、换页面、重启应用都是瞬间出数据，右上角显示「更新中」而不是整屏 loading
- 数据超过 1 分钟才会挂一句「N 分钟前的数据」；刚采完不显示，避免每次进页面都多一句废话
- **体积增量测量**：`du` 的耗时是本地数据里最大的一块（46 个 npm 包 7.2s）。现在按路径记录测量时间，只量**没量过或超过 10 分钟**的路径；只有点「刷新」才强制全量重量
- **版本变了自动重量**：重扫时若发现某个包版本号变了（升级 / 重装过），只把它的体积作废重量，其它沿用缓存——实测切一次视图回来只重扫 1 条路径
- 已从列表消失的路径会从缓存里剔除，避免缓存越滚越大。剔除放在每次采集之后（`evictSizes()`），而不是塞在体积测量里——测量在"没有过期项"时会提前返回，塞在里面等于卸载过的包体积永远清不掉

## 7. 设置与打磨

- [x] 设置页：**包数据源与代理可编辑**（`src/views/SettingsView.vue` + `src/composables/useSettings.ts`）。含「测试连接」（用草稿值探测，可先测再存）、「放弃改动」、「恢复默认」
- [x] 数据源与代理贯通到真实请求：搜索 / 详情 / 版本历史 / README / 批量最新版本 / 诊断自检 / brew 安装量统计
- [x] 安装类命令一并生效：`NPM_CONFIG_REGISTRY`、`HTTPS_PROXY` + `HTTP_PROXY`、跳过证书时 `NODE_TLS_REJECT_UNAUTHORIZED=0` 通过环境变量传给子进程（`actions::env_for`，有单测）；确认框里展示的 `display` 就带这些前缀
- [x] 命令试跑（诊断页）也带代理与证书开关：`brew update` 这类命令在访达启动的 GUI 里同样拿不到终端变量
- [ ] 设置页：默认包管理器（当前安装方式默认为第一个可用项）
- [x] 持久化：设置存 `localStorage`（与主题同一套做法）。后端不持有配置状态，每次调用显式传参，避免"改了设置但某模块还读旧值"
- [ ] 错误兜底：管理器未安装、网络失败、权限不足分别给出明确的成因与恢复路径
- [ ] 打包分发：macOS 签名与公证需要 Apple Developer 账号，提前预留
- [x] 产品命名：**Pantry**（`productName: Pantry` / `identifier: com.hengran.pantry` / 窗口标题 `Pantry` / 侧栏字标 `./pantry` / `design-system/pantry/` / 发往 registry 的 user agent `pantry/0.1`）。**刻意不变**的内部标识：crate 与二进制名 `hengran-public-store`、`package.json` 的 `name`、仓库目录名、`localStorage` 前缀 `public-store:*`、shell_env 哨兵串——它们对用户不可见，改了还会丢设置与缓存、并让 ROADMAP 里那条 `--diagnose` 实测记录失效
- [ ] 应用图标、版本号（仍为 0.1.0；图标还是 Tauri 默认图标）

### 设置项的作用范围（已固化）

- **留空 = 不干预**：registry 留空用后端内置默认源（`market::DEFAULT_REGISTRY`，由 `market_defaults` 提供给界面做占位），代理留空按「终端环境变量 → macOS 系统设置 → 直连」自动解析
- **不改写用户的 dotfile**：设置只影响本应用发起的请求与它 spawn 的子进程，不写 `.npmrc` / `~/.bunfig.toml`
- **brew 只吃代理**：它的"源"是 tap 仓库而不是 registry，证书开关对 Ruby 侧也没有意义，所以 `env_for` 对 brew 只注入代理；搜索、详情、安装三条路都带（`brew_env`），不会出现"装得上、搜不到"
- **下载量与 README 不受 registry 影响**：它们分别走 `api.npmjs.org` 与 `cdn.jsdelivr.net`，界面里如实标注，避免"换了镜像为什么数字没变"的困惑。**代理对这两个域名同样生效**（`fetch_downloads` 跟随设置），文案里写清楚了

### 代码评审后的修复（本轮）

- [x] **已安装列表的版本比较接回后端**：原来 `latest !== item.version` 会把"本机装了 canary / 更高版本"误报成可升级（点下去是降级）。现在前端把 `{name, local, latest}` 交给新命令 `newer_versions`，与诊断自检共用 `managers::compare_versions`，结论随缓存一起存（`newer` 字段）；有单测覆盖 canary / 同号预发布两种情况
- [x] **命令面板的可访问性**：输入框改成 `role="combobox"` + `aria-activedescendant` + `aria-controls`，列表是 `role="listbox"` / `role="option"` + `aria-selected`；焦点环由输入行 `focus-within` 描出（与 AppSearchField 同一套做法），不再有"移除焦点指示"的写法
- [x] **`useScrollLock` 改成引用计数 + 卸载归还**：命令面板与页面弹层可能同时开着，先关的那个不再把另一个的锁解掉；弹层开着被卸载也会把锁还回去
- [x] **中文标签不再套等宽**：`AppButton` / `AppTag` / `StatusBadge` / 搜索按钮按 `tokenFont()` 判断，拉丁与数字才加 `font-mono`（MASTER.md 的反模式条目）
- [x] **README 请求加守卫**：与详情、版本历史一致，A 的慢响应不会再落到已经切到 B 的界面上；搜索摘要补上「来自缓存」，把这个字段用起来
- [x] **删掉不可达的 `embedded=false` 分支**（InstalledView / MarketView / BrewView）：三个视图只作为生态页的内嵌视图存在，自带大标题与页边距的分支是独立页面被删掉后的残骸
- [x] **`cask_app_path` 只留一份**（brew.rs 提供，packages.rs 复用）
- [x] **brew 搜索不再静默丢名字**：超过 30 个的匹配仍以 name-only 返回（之前直接消失，用户既看不到也点不进去）
- [x] **缓存剔除时机**：`evictSizes()` 移到每次采集之后（`measure()` 没有过期项时会提前返回，剔除去不掉）
- [x] **`.gitignore` 放行 `design-system/`**：MASTER.md 是 ROADMAP 引用的绑定设计标准，不该被忽略

- [x] **修一个会让命令静默不执行的缺陷**：`shell_join` 与管理员路径的变量前缀都少了一个分隔符，拼出来是 `export PATH=... 'brew' 'install' 'docker'`——`export` 是普通命令，后面的词会被当成要导出的变量名，程序根本没跑。本机 `/bin/sh`（`osascript` 用的就是它）实测两种表现：参数里带 `-` 时报 `not a valid identifier` 退出 1，全为合法标识符时**静默退出 0**（界面报成功但什么都没装）。现在统一写成 `export ...; 命令`，并新增 `admin_shell_string_really_runs_the_command`：把 brew 换成 `/bin/echo` 后用真实 `/bin/sh` 跑一遍，再用临时改坏代码的方式确认它能拦住旧写法（旧写法下该测试失败）。测试里**先断言替换确实生效、再交给 shell**：否则哪天拼接方式一变，这个单测就会真的执行 `brew install --cask docker`
- [x] **brew 搜索 / 详情也带代理**：新增 `brew_env(proxy)`，`run_brew` / `search_names` / `info_batch` 全部走它（有单测钉住 `HOMEBREW_NO_AUTO_UPDATE` 仍在、空白代理按未设置处理）
- [x] **「恢复默认」改为只改草稿**：不再绕过「保存设置」直接写 localStorage，误点不会悄悄覆盖已保存的配置；草稿已是默认值时按钮置灰（判断要把证书开关算进去，否则「只打开了开关」那种状态下按钮会点了没反应）
- [x] **`AppSwitch` 整行可点**：原来只有滑块与「开 / 关」文字是点击目标，标题与说明点了没反应（注释却写着整行可点）。现在整行是**一个** `button`（不嵌套按钮），标题与说明分别用 `aria-labelledby` / `aria-describedby` 关联
- [x] **弹窗不再被粘性表头盖住**：详情面板位于 `sticky` 列，而 `sticky` 自成层叠上下文，`fixed inset-0 z-1000` 的弹窗被困在里面，被列表的粘性表头（`z-10`）横穿。升级确认框与 README 弹层改为 `<Teleport to="body">`，并把这条件写进 MASTER.md 的已知陷阱（实测方式：表头与弹窗重叠处用 `elementsFromPoint` 判断命中谁）

### 外链与包元数据（本轮）

- [x] **主页 / 仓库可直接点开**：详情里的地址不再是纯文本，点击用**系统默认浏览器**打开（`open_external_url` → opener 插件）
- **为什么不自建窗口（踩过一次）**：先做成了应用内的独立窗口（`WebviewWindowBuilder` + `WebviewUrl::External`），实测两个问题——那扇窗里没有用户的登录态，也用不上他自己配的代理与浏览器扩展，需要 VPN 或需要登录的站点（GitHub、npm 官网）直接打不开；而且它只是又一扇要管理的窗。系统浏览器才是用户已经配好的环境，代理、登录、多标签都在。子 webview 与 iframe 更早就被排除了：multiwebview 还在 `unstable` 特性后面，iframe 会被 `X-Frame-Options` 挡掉
- **安全边界**：命令只放行 `http` / `https`，`file:` 与自定义 scheme 一律拒绝，免得"打开外链"变成任意本地文件或任意协议的入口（`link_target` 有单测）
- [x] **已安装包补上说明 / 主页 / 仓库**：npm 系的列表命令只返回名字与版本，这三项改为读各包自己的 `package.json`（本机 46 个包毫秒级，且不依赖 registry 可达性）；deno 的全局目录里没有 package.json，所以它没有这三项。brew 的 JSON 没有 repository 字段，取 `urls.head`，且只在"看起来像仓库"时才算（以 `.git` 结尾或落在常见代码托管站）——本机实测 113 个 brew 包里 52 个有仓库地址
- [x] **地址归一化收进 `src-tauri/src/links.rs`**：`git+https://…`、`{type,url}` 对象、`github:user/repo`、`git@github.com:user/repo.git` 统一转成能点开的 `https://…`；转不成的一律不显示，界面不给点了没反应的链接
- [x] **市场页与 Homebrew 页详情同步可点**（原来只有已安装详情有主页，且是纯文本）

## 建议落地顺序

先打通最小闭环：**npm 与 pnpm 已装列表 → npmmirror 搜索 → 选择管理器安装 → 流式日志**。这条链路同时验证 PATH、网络、子进程三个难点。随后补 Tailwind 与双主题基建，再扩展 bun 与 deno，brew 放最后（涉及 sudo 与慢速更新）。
