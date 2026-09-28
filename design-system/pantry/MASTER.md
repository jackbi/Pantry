# Design System Master File

> **LOGIC:** 构建具体页面前，先查 `design-system/pantry/pages/[page-name].md`。
> 存在则其规则**覆盖**本文件；不存在则严格遵循本文件。

**Project:** Pantry（Tauri 桌面应用 / macOS 优先）
**Category:** Developer Tool
**基线技能:** `ui-ux-pro-max`
**视觉参考:** [WailBrew](https://www.wailbrew.app/)（信息架构）、[npmx](https://npmx.dev/)（排版纪律）

---

## 平台与基调

桌面工具，不是营销页。主要交互是浏览列表、搜索、查看日志，信息密度偏高。

- **Style Baseline:** Minimalism & Swiss Style（数据集标注明暗双支持、复杂度低、兼容 tailwind）
- 该风格的关键词直接决定了几条硬规则：**不用阴影、不用渐变、细边框、高对比、克制的色彩**
- **不采用**数据集为 Developer Tool 推荐的 slate 暗色板：标注 `Light Mode: not-recommended`，与双主题要求冲突
- 密度：控件高度 32px，列表行高 36px，正文 13px

## 两个参考站给出的设计语言

| 来源 | 借鉴点 |
|------|--------|
| WailBrew | 分组侧栏（PACKAGES / BROWSING / TOOLS）带计数、可排序密集表格（NAME / VERSION / SIZE / ACTIONS）、行内操作按钮、筛选片、表格下方详情面板与空态、深色底 + 细边框 |
| npmx | 等宽字体当主声音、`jump to… ⌘K` 键帽、`/` 前缀搜索框、极简顶栏、单一强调色（黑底白字按钮 + 蓝色焦点）、`•标签` 生态标签排、大量留白 |

组合后的结论：**信息架构学 WailBrew，排版与克制学 npmx**。

## 主题策略

- 明亮与黑暗同时是一等公民，两套都要过对比度检查
- 跟随系统 + 手动切换（亮 / 暗 / 跟随），结果持久化
- Tailwind class 策略：`@custom-variant dark (&:where(.dark, .dark *))`
- 挂载前注入主题 class，避免白闪；并同步 Tauri 窗口标题栏
- 暗色是独立色阶取值，不是反相

## Color Tokens

中性黑白 + 单一蓝色强调。组件里只允许写语义类名（`bg-surface`），禁止硬编码色值。

### 明亮

| Token | 值 | 用途 |
|-------|-----|------|
| `--color-background` | `#FCFCFD` | 页面底色 |
| `--color-surface` | `#FFFFFF` | 卡片、表格、面板 |
| `--color-muted` | `#F4F4F5` | 次级区块、日志底、行悬停 |
| `--color-foreground` | `#0A0A0B` | 正文 |
| `--color-muted-foreground` | `#52525B` | 次要文字、分组标题 |
| `--color-border` | `#E4E4E7` | 装饰分隔线、卡片描边 |
| `--color-control-border` | `#71717A` | 输入框与按钮边界（需 3:1） |
| `--color-primary` | `#18181B` | 主操作（黑底白字，参考 npmx 的 search 按钮） |
| `--color-primary-foreground` | `#FFFFFF` | 主操作前景 |
| `--color-accent` | `#1D4ED8` | 链接、当前项指示条、焦点环 |
| `--color-danger` | `#B91C1C` | 危险操作、错误 |
| `--color-ring` | `#1D4ED8` | 焦点环 |

### 黑暗

| Token | 值 | 用途 |
|-------|-----|------|
| `--color-background` | `#09090B` | 页面底色 |
| `--color-surface` | `#17171A` | 卡片、表格、面板 |
| `--color-muted` | `#232327` | 次级区块、日志底、行悬停 |
| `--color-foreground` | `#FAFAFA` | 正文 |
| `--color-muted-foreground` | `#A1A1AA` | 次要文字 |
| `--color-border` | `#2E2E34` | 装饰分隔线 |
| `--color-control-border` | `#77777F` | 控件边界（需 3:1，比初版提亮一档才达标） |
| `--color-primary` | `#FAFAFA` | 主操作（白底黑字） |
| `--color-primary-foreground` | `#09090B` | 主操作前景 |
| `--color-accent` | `#60A5FA` | 链接与当前项指示 |
| `--color-danger` | `#F87171` | 危险操作 |
| `--color-ring` | `#60A5FA` | 焦点环 |

### 已实测对比度（脚本计算，非估算）

| 组合 | 明亮 | 黑暗 | 要求 |
|------|------|------|------|
| 正文 / 表面 | 19.79:1 | 17.14:1 | ≥4.5 |
| 次要文字 / 表面 | 7.73:1 | 6.98:1 | ≥4.5 |
| 主按钮前景 / 主色 | 17.72:1 | 19.06:1 | ≥4.5 |
| 强调色 / 表面 | 6.70:1 | 7.04:1 | ≥4.5 |
| 危险色 / 表面 | 6.47:1 | 6.47:1 | ≥4.5 |
| 控制边框 / 表面 | 4.83:1 | 3.39:1 | ≥3 |

装饰分隔线（`--color-border`）不适用 3:1，只需两套主题下都可见。

## Typography

技能给出的配对是 **Developer Mono**：`JetBrains Mono`（标题/代码）+ `IBM Plex Sans`（正文）。

**中文适配（重要）**：JetBrains Mono 不含中文字形，对中文标签用等宽等于没生效，只会回退到系统字体。因此：

- **等宽**只用于拉丁与数字：包名、版本号、路径、命令、计数、键帽、版本标记
- **中文标签**用无衬线，靠字号 / 字距 / 字重区分层级（`label-mini` 工具类：11px / 500 / 0.06em）
- 字号刻度：`--text-caption 11 / label 12 / body 13 / title 14 / heading 20`
- 数据列使用 `tabular-nums`，避免数字跳动

## Spacing、圆角与层级

- 4/8px 节奏；面板内边距 12px，侧栏项目高度 32px
- 圆角刻意做小：`rounded-control` 4px、`rounded-card` 6px（参考站均为小圆角）
- 无阴影、无渐变；层次只靠边框与底色差
- z-index 分层：`0 / 10 / 20 / 40 / 100 / 1000`（浮层用 1000）

## 组件层

| 组件 | 路径 | 职责 |
|------|------|------|
| `AppButton` | `ui/AppButton.vue` | primary / secondary / ghost / danger，内置 loading 与四态。标签**不套等宽**（按钮文案以中文为主），需要等宽的拉丁标签由调用方传 `font-mono` |
| `AppInput` | `ui/AppInput.vue` | 强制可见 label，hint / error 走 aria-describedby |
| `AppPanel` | `ui/AppPanel.vue` | 标题 + 说明 + 操作区 |
| `StatusBadge` | `ui/StatusBadge.vue` | 状态强制"图标 + 文字"；标签按 `tokenFont()` 判断，含中文就不套等宽 |
| `LogViewer` | `ui/LogViewer.vue` | 命令输出，`role="log"` + `aria-live`，跟随滚动 |
| `AppTable` | `ui/AppTable.vue` | 密集表格，可排序表头带 `aria-sort`，粘性表头，行内操作插槽 |
| `AppDetailPane` | `ui/AppDetailPane.vue` | 表格下方详情区，未选中时给空态 |
| `AppEmptyState` | `ui/AppEmptyState.vue` | 空态：图标 + 标题 + 说明 + 操作 |
| `AppExternalLink` | `ui/AppExternalLink.vue` | 主页 / 仓库外链：点开应用内的独立窗口（后端只放行 http(s)），窗口标题写成「包名 · 主页」 |
| `AppSearchField` | `ui/AppSearchField.vue` | `/` 前缀 + 键帽 + 提交按钮的搜索入口 |
| `AppTag` | `ui/AppTag.vue` | 标签 / 筛选片，`•` 前缀，传 active 才渲染为按钮；包名等拉丁标签自动用等宽，中文标签用正文字体 |
| `AppSwitch` | `ui/AppSwitch.vue` | 布尔开关，**整行可点**（标题、说明、滑块同属一个按钮，点哪都切换），`role="switch"` + `aria-labelledby/describedby` + 原生键盘行为；有风险的开关用 `tone="danger"`，滑块位置之外还有「开 / 关」文字 |
| `CommandPalette` | `CommandPalette.vue` | ⌘K 跳转面板。输入框是 `role="combobox"` + `aria-activedescendant` 指向当前高亮项，列表是 `role="listbox"` 内的 `role="option"`；焦点环由输入行 `focus-within` 描出 |
| `AppKeycap` | `ui/AppKeycap.vue` | 快捷键键帽 |
| `AppSidebar` | `AppSidebar.vue` | 分组导航，当前项用底色 + 强调色指示条 + `aria-current` |
| `CommandPalette` | `CommandPalette.vue` | ⌘K 跳转面板，方向键 + Enter + Esc |
| `ThemeSwitcher` | `ThemeSwitcher.vue` | 三态主题，图标 + `aria-label` + `aria-pressed` |

### 工具类

| 名称 | 定义 | 用途 |
|------|------|------|
| `wrap-token` | `min-inline-size: 0; overflow-wrap: anywhere` | 长路径、URL、包名换行 |
| `keycap` | 键帽样式 | 快捷键提示 |
| `label-mini` | 11px / 500 / 0.06em / muted | 分组标题、表头、面板小标题 |
| `tokenFont()` | `src/lib/text.ts` | 拉丁与数字返回 `font-mono`，含中文返回空串——等宽只用于拉丁，别对中文标签套等宽 |

## 反模式（禁止）

- 组件里硬编码 hex 或 `bg-slate-800` 这类调色板类名，绕过语义 token
- `outline: none` 或任何移除焦点指示的写法（唯一例外见下）
- 用阴影、渐变、大圆角制造"层次"
- 只用颜色传达状态（必须同时有图标或文字）
- 仅靠 hover 才能触发的操作；placeholder 代替 label
- 给中文标签套 `font-mono` 或 `uppercase`
- 在组件里写全局 `<style>` 块（会把 token 泄漏到全局）

### 已记录的例外

- `src/App.vue` 的主内容区 `<main tabindex="-1">` 使用 `focus:outline-none`。它是跳转链接的目标而非可交互控件，程序化聚焦时给整个内容区描环会造成视觉噪声。

## 已知陷阱

### sr-only 必须有已定位的祖先

Tailwind 的 `sr-only` 用 `position: absolute`。若它没有已定位的祖先，包含块会变成**初始包含块**，于是元素按静态位置被放到文档坐标系里——写在可滚动内容深处的 `<caption class="sr-only">` 会直接在外层撑出滚动条（实测把 520px 的视口撑到 634px）。

因此滚动容器 `<main>` 必须带 `relative`。新增任何滚动容器时同理。

### 并用 items-end 与不等高的表单列

一行里若某一列多一行说明文字（hint），各列高度就不同，`items-end` 会按**底边**对齐，导致带 hint 的那一列输入框被顶上去（实测差 20px）。解决方式是把说明文字放在整行下方，让各列结构一致。

### 数据语义不明时不要给对称的视觉提示

升级提示不能无脑渲染成 `旧 → 新`：当上游只是"需要重建"而版本号未变时，会出现 `23.1.1 → 23.1.1` 这种既无意义又像 bug 的显示。规则是先判断两个值是否真的不同，不同才给箭头，相同则改用另一种措辞（如"可重建"）并说明原因。

### 主滚动容器只能有一个

页面滚动发生在 `<main>` 上，任何再套一层 `max-h + overflow-y-auto` 都会并列出现两根滚动条。这一条已经踩过两次：

- 侧栏常驻：`sticky top-4` 就够了，再加高度限制与内滚就会和主滚动打架
- 长内容（如 README）：放进模态，并用 `useScrollLock` 锁住 `<main>`，弹层期间只保留弹层自己的滚动

### 表头与数据行不能各自成格

紧凑表格若用两个独立的 `grid`（一个渲染表头、一个渲染数据行），列宽会各算各的：数据行里「下载量」列内容窄，列就窄；表头写着「7 日下载」，那一列被撑宽——同一套 `grid-cols-[...]` 模板也会错开一个版本列的宽度（实测 44px vs 6.6px）。且 `minmax(0,1fr)` 会让版本列默默吃掉差值，看起来「没报错但没对齐」。

固定列宽的场景用真正的 `<table class="w-full table-fixed">` + `<colgroup>`，表头与数据行共享同一套列宽；版本号那种不定长内容配 `max-w-0 truncate` + `title`，避免长版本号（如 `0.1.7-alpha.1+build.20260922123045…`）撑破布局。

## 交付前检查清单

- [ ] 亮色与暗色**分别**验证：正文、次要文字、按钮、状态色、边框、焦点环
- [ ] 键盘可达：Tab 顺序与视觉顺序一致，焦点不被浮层遮挡
- [ ] 可点元素具备 hover / focus-visible / active / disabled 四态
- [ ] 无 emoji 图标；图标语义正确（装饰 vs 有意义）
- [ ] `prefers-reduced-motion` 下动效降级
- [ ] 长文本、长包名、长路径不撑破布局（`wrap-token`）
- [ ] 空态有引导与下一步动作，不留白
