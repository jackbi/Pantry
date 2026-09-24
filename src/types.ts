// 与 Rust 侧 serde 结构体一一对应（字段名由后端 rename_all = "camelCase" 保证）

// 按生态分页：Node 包（npm/pnpm/bun/deno）与 Homebrew，每页各自「已安装 / 安装新包」
export type NavKey = "node" | "brew" | "diagnostics" | "settings";

export type NavGroup = { label: string; items: { key: NavKey; label: string }[] };

/** 表格列定义，供 AppTable 使用 */
export type TableColumn = {
  key: string;
  label: string;
  sortable?: boolean;
  align?: "left" | "right";
  width?: string;
  mono?: boolean;
};

export type TableRow = { id: string; [key: string]: unknown };

export type InstalledPackage = {
  name: string;
  /** 无法确定版本时为空字符串（deno 的安装脚本没有版本元数据） */
  version: string;
  source: string;
  description: string | null;
  homepage: string | null;
  kind: string | null;
  path: string | null;
  /** cask 安装出来的 .app 路径，用于「打开」与「在访达中显示」 */
  appPath: string | null;
  outdated: boolean;
  /** outdated 但版本号未变：Homebrew formula 修订号更新，升级等于按新配方重建 */
  revisionOutdated: boolean;
  latest: string | null;
};

export type SourceError = { source: string; message: string };

export type InstalledReport = {
  packages: InstalledPackage[];
  errors: SourceError[];
};

export type SizeEntry = { path: string; bytes: number };

export type LatestVersion = {
  name: string;
  latest: string | null;
  error: string | null;
};

/** 交给后端比较的一对版本：本机装的 vs registry 上的 latest */
export type VersionProbe = { name: string; local: string; latest: string };

export type VersionVerdict = { name: string; newer: boolean };

export type BrewKind = "formula" | "cask";

export type BrewSearchItem = {
  name: string;
  kind: BrewKind;
  version: string | null;
  description: string | null;
  homepage: string | null;
  installed: boolean;
  outdated: boolean;
  latest: string | null;
};

export type BrewSearchResponse = {
  query: string;
  items: BrewSearchItem[];
  /** 单种类型失败时如实带回原因，另一种仍可用 */
  errors: string[];
  tookMs: number;
};

export type BrewDetail = {
  name: string;
  kind: BrewKind;
  displayName: string | null;
  version: string | null;
  description: string | null;
  homepage: string | null;
  license: string | null;
  tap: string | null;
  installed: string | null;
  latest: string | null;
  outdated: boolean;
  pinned: boolean;
  dependencies: string[];
  conflicts: string[];
  caveats: string | null;
  deprecation: string | null;
  appPath: string | null;
  autoUpdates: boolean;
};

export type BrewStats = {
  name: string;
  installs30d: number | null;
  error: string | null;
};

export type BrewAvailability = {
  /** `macos` / `linux` / … */
  platform: string;
  available: boolean;
  path: string | null;
  version: string | null;
};

export type ManagerStatus = {
  name: string;
  available: boolean;
  path: string | null;
  version: string | null;
  /** registry 上的最新版本；不参与对比或取不到时为 null */
  latest: string | null;
  upgradeAvailable: boolean;
  /** true 表示这个工具自己更新自己（brew），界面不显示"可升级" */
  selfUpdating: boolean;
  /** 更新命令；node 没有（升级走 nvm / Homebrew 等） */
  update: UpdateCommand | null;
  error: string | null;
};

export type UpdateCommand = { program: string; args: string[]; display: string };

export type PackageLinks = {
  npm: string | null;
  homepage: string | null;
  repository: string | null;
  issues: string | null;
};

export type SearchHit = {
  name: string;
  version: string;
  description: string | null;
  publishedAt: string | null;
  publisher: string | null;
  keywords: string[];
  links: PackageLinks;
  score: number | null;
  /** 已发布版本总数，来自搜索响应；缺失时为 null */
  versionCount: number | null;
};

export type SearchResponse = {
  query: string;
  total: number;
  hits: SearchHit[];
  tookMs: number;
  cached: boolean;
};

export type Dependency = { name: string; range: string };

export type DistTag = { tag: string; version: string };

export type PackageDetail = {
  name: string;
  version: string;
  description: string | null;
  license: string | null;
  homepage: string | null;
  repository: string | null;
  unpackedSize: number | null;
  fileCount: number | null;
  dependencies: Dependency[];
  peerDependencies: Dependency[];
  keywords: string[];
  deprecated: string | null;
  weeklyDownloads: number | null;
  /** 近 7 天每日下载量，用于趋势图 */
  weeklySeries: number[];
  /** 下载量口径：npm 为官方，npmmirror 为镜像（数值明显偏低） */
  downloadsSource: string | null;
  downloadsError: string | null;
  distTags: DistTag[];
};

export type PackageVersion = {
  version: string;
  /** 命中该版本的 dist-tag，如 latest */
  tags: string[];
  publishedAt: string | null;
  downloadsLastWeek: number | null;
  deprecated: string | null;
};

export type VersionHistory = {
  name: string;
  total: number;
  lastPublishedAt: string | null;
  versions: PackageVersion[];
  /** 版本数超过上限时为 true，只回传最近的一段 */
  truncated: boolean;
  downloadsSource: string | null;
  downloadsError: string | null;
};

export type ReadmeResponse = {
  name: string;
  version: string;
  markdown: string;
  sourceUrl: string;
};

export type PackageAction = "install" | "uninstall" | "upgrade";

export type PlannedCommand = {
  source: string;
  action: PackageAction;
  /** 需要管理员权限时会是 osascript */
  program: string;
  args: string[];
  /** 随命令一起传给包管理器的环境变量（数据源、代理、证书开关） */
  env: { key: string; value: string }[];
  /** 展示给用户确认的完整命令 */
  display: string;
  requiresAdmin: boolean;
  destructive: boolean;
};

export type PathReport = {
  shell: string;
  shellPath: string | null;
  resolved: string[];
  fallbacksAdded: string[];
  source: string;
};

export type ProxyReport = {
  proxy: string | null;
  source: string;
  fromEnv: string | null;
  fromSystem: string | null;
};

/** 设置页里的网络配置：留空表示用内置默认源 / 自动探测代理 */
export type MarketSettings = {
  registry: string;
  proxy: string;
  insecure: boolean;
};

/** 传给 market / probe 类命令的参数，空值一律转成 null */
export type RequestOptions = {
  registry: string | null;
  proxy: string | null;
  insecure: boolean;
};

/** 后端内置默认值，设置页的占位与「恢复默认」都读它 */
export type MarketDefaults = {
  registry: string;
  downloadsApi: string;
  readmeCdn: string;
};

export type ProbeResult = {
  url: string;
  ok: boolean;
  status: number | null;
  elapsedMs: number;
  proxy: string | null;
  insecure: boolean;
  sample: string | null;
  error: string | null;
};

export type ProcEvent =
  | {
      kind: "started";
      id: string;
      program: string;
      executable: string;
      args: string[];
      pid: number;
    }
  | { kind: "output"; id: string; stream: string; line: string }
  | {
      kind: "exit";
      id: string;
      code: number | null;
      killed: boolean;
      elapsedMs: number;
    };
