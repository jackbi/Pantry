// 已安装列表的本地缓存：先把上次结果留着，再在后台更新。
//
// 为什么需要：底层命令（`npm ls -g`、`brew info --json=v2 --installed`）只能返回全量状态，
// 没有增量接口。而本机实测 npm 那次调用 2.6s、全部包的 `du` 分片后仍要 2.8s——切一次视图、
// 换一次页面、重启一次应用就把这些都重跑一遍，纯属白等。
//
// 所以这里把上次结果留在本机（localStorage + 内存），打开时先渲染旧数据，
// 同时后台刷新；体积单独记录测量时间，只补没量过或过期的路径（增量测量）。
import type { InstalledPackage, SourceError } from "../types";

export type InstalledCacheEntry = {
  /** 上次采集完成的时间 */
  at: number;
  packages: InstalledPackage[];
  errors: SourceError[];
  sizes: Record<string, number>;
  /** 每个路径的体积是何时测的，用于增量测量 */
  sizesAt: Record<string, number>;
  latest: Record<string, string>;
  /**
   * registry 上的 latest 是否**确实**比本机版本新（后端比较器的结论）。
   * 旧缓存没有这个字段，读出来是 undefined，交给调用方兜底成空表。
   */
  newer?: Record<string, boolean>;
};

/** 体积多久算旧。过期才重量，避免每次进页面都把几万个文件再 stat 一遍 */
export const SIZE_TTL_MS = 10 * 60 * 1000;

const STORAGE_PREFIX = "public-store:installed:";
const memory = new Map<string, InstalledCacheEntry>();

export function readInstalledCache(scope: string): InstalledCacheEntry | null {
  const cached = memory.get(scope);
  if (cached) return cached;
  try {
    const raw = localStorage.getItem(STORAGE_PREFIX + scope);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as InstalledCacheEntry;
    // 结构不对就当没有缓存：坏数据不该让页面卡在渲染上
    if (typeof parsed?.at !== "number" || !Array.isArray(parsed?.packages)) return null;
    memory.set(scope, parsed);
    return parsed;
  } catch {
    return null;
  }
}

export function writeInstalledCache(scope: string, entry: InstalledCacheEntry) {
  memory.set(scope, entry);
  try {
    localStorage.setItem(STORAGE_PREFIX + scope, JSON.stringify(entry));
  } catch {
    // 配额满或隐私模式：内存缓存仍然有效，不影响本次使用
  }
}
