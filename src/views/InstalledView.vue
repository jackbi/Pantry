<script setup lang="ts">
// 已安装列表：真实数据。并行采集 + 体积增量测量 + 来源筛选 + 排序 + 详情面板。
//
// 这个组件被两个生态页复用：
// - scope="node"：只采集 npm / pnpm / bun / deno（不再等 brew 的一两秒）
// - scope="brew"：只采集 brew，并把「来源」列换成「类型」（formula / cask）
// 只作为生态页（Node 包 / Homebrew）里的一个视图使用，标题与页边距由外层页面提供。
//
// 数据策略是"先保留再更新"：底层命令只有全量接口，所以把上次结果缓存在本机，
// 打开时先渲染旧数据、同时后台刷新；体积按路径记录测量时间，只补没量过或过期的。
import { computed, onMounted, ref, watch } from "vue";
import {
  PhArrowClockwise,
  PhArrowSquareOut,
  PhFolderOpen,
  PhPackage,
  PhWarningCircle,
} from "@phosphor-icons/vue";
import { invoke } from "@tauri-apps/api/core";

import AppButton from "../components/ui/AppButton.vue";
import AppDetailPane from "../components/ui/AppDetailPane.vue";
import AppEmptyState from "../components/ui/AppEmptyState.vue";
import AppSearchField from "../components/ui/AppSearchField.vue";
import AppTable from "../components/ui/AppTable.vue";
import AppTag from "../components/ui/AppTag.vue";
import StatusBadge from "../components/ui/StatusBadge.vue";
import ActionPanel from "../components/ActionPanel.vue";
import { readInstalledCache, SIZE_TTL_MS, writeInstalledCache } from "../composables/useInstalledCache";
import { useSettings } from "../composables/useSettings";
import { isTauri } from "../lib/tauri";
import type {
  InstalledPackage,
  InstalledReport,
  LatestVersion,
  PackageAction,
  SourceError,
  SizeEntry,
  TableColumn,
  TableRow,
  VersionProbe,
  VersionVerdict,
} from "../types";

const props = withDefaults(defineProps<{ scope?: "node" | "brew" }>(), { scope: "node" });

// 版本比对要跟设置页里的数据源 / 代理走，否则会出现"市场按镜像查、升级提示按官方查"
const { requestOptions } = useSettings();

const SOURCES: Record<"node" | "brew", string[]> = {
  node: ["npm", "pnpm", "bun", "deno"],
  brew: ["brew"],
};

const columns = computed<TableColumn[]>(() => {
  const middle: TableColumn =
    props.scope === "brew"
      ? { key: "kind", label: "类型", sortable: true, mono: true, width: "5rem" }
      : { key: "source", label: "来源", sortable: true, mono: true, width: "5rem" };
  return [
    { key: "name", label: "名称", sortable: true, mono: true },
    { key: "version", label: "版本", sortable: true, mono: true, align: "right", width: "8rem" },
    middle,
    { key: "size", label: "体积", sortable: true, mono: true, align: "right", width: "6rem" },
  ];
});

const showSourceFilter = computed(() => props.scope !== "brew");

// 文案跟着 scope 走：Node 页根本不碰 brew，就不能说"brew 通常需要一两秒"
const copy = computed(() =>
  props.scope === "brew"
    ? {
        caption: "已安装的 Homebrew 包",
        subtitle: "本机 Homebrew 安装的 formula 与 cask。",
        loading: "正在读取 brew 已安装的 formula 与 cask…",
        emptyTitle: "没有采到任何 Homebrew 包",
        emptyHint: "确认 brew 可用，或点右上角刷新重试。",
        filterHint: "换个关键词。",
      }
    : {
        caption: "已安装的全局包",
        subtitle: "npm、pnpm、bun、deno 的全局包，体积为磁盘占用。",
        loading: "正在并行读取 npm、pnpm、bun、deno…",
        emptyTitle: "没有采到任何全局包",
        emptyHint: "确认至少装了一个（npm / pnpm / bun / deno），或点右上角刷新重试。",
        filterHint: "换个关键词或把来源筛选切回「全部」。",
      },
);

const cached = readInstalledCache(props.scope);
/** 是否已经有过一次完整结果（本次或上次运行留下的） */
const loaded = ref(cached !== null);
/** 首次采集（本机没有缓存）才显示"正在采集"；有缓存时是静默的后台更新 */
const loading = ref(false);
const refreshing = ref(false);
const measuring = ref(false);
const packages = ref<InstalledPackage[]>(cached?.packages ?? []);
const errors = ref<SourceError[]>(cached?.errors ?? []);
const sizes = ref<Record<string, number>>(cached?.sizes ?? {});
const sizesAt = ref<Record<string, number>>(cached?.sizesAt ?? {});
// npm 系（npm/pnpm/bun）的最新版本要靠 registry 才知道，brew 自带 outdated 信息
const latestByName = ref<Record<string, string>>(cached?.latest ?? {});
const newerByName = ref<Record<string, boolean>>(cached?.newer ?? {});
const collectedAt = ref<number | null>(cached?.at ?? null);
const checkingLatest = ref(false);
const loadError = ref<string | null>(null);

const query = ref("");
const source = ref("全部");
const sortKey = ref<string | null>("name");
const sortDir = ref<"asc" | "desc">("asc");
const selected = ref<InstalledPackage | null>(null);
const installedAction = ref<PackageAction>("upgrade");
const opening = ref<"open" | "reveal" | null>(null);
const openError = ref<string | null>(null);

/**
 * registry 上真正更新的版本。
 *
 * 判定不看"是否相等"，而是看后端比较器给的结论：本机装了 canary 或更高版本时，
 * 简单的不相等会把**降级**误报成可升级。
 */
function newerVersion(item: InstalledPackage): string | null {
  if (item.source === "brew") return item.latest;
  if (!item.version) return null;
  const latest = latestByName.value[item.name];
  return latest && newerByName.value[item.name] ? latest : null;
}

const upgradableCount = computed(
  () => packages.value.filter((item) => newerVersion(item) !== null).length,
);

// brew 的 upgrade 不接受版本号，npm 系要显式指定 latest 才会真的升级
const upgradeVersion = computed(() => (selected.value?.source === "brew" ? null : "latest"));

function formatSize(bytes: number | undefined): string {
  if (bytes === undefined) return measuring.value ? "…" : "—";
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function versionText(item: InstalledPackage): string {
  return item.version || "—";
}

const sourceCounts = computed(() => {
  const counts = new Map<string, number>();
  for (const item of packages.value) {
    counts.set(item.source, (counts.get(item.source) ?? 0) + 1);
  }
  return counts;
});

const sourceFilters = computed(() => {
  const names = [...sourceCounts.value.keys()].sort();
  return [
    { name: "全部", count: packages.value.length },
    ...names.map((name) => ({ name, count: sourceCounts.value.get(name) ?? 0 })),
  ];
});

const visible = computed(() => {
  const keyword = query.value.trim().toLowerCase();
  const filtered = packages.value.filter((item) => {
    if (source.value !== "全部" && item.source !== source.value) return false;
    if (!keyword) return true;
    return (
      item.name.toLowerCase().includes(keyword) ||
      (item.description ?? "").toLowerCase().includes(keyword)
    );
  });

  const key = sortKey.value;
  if (!key) return filtered;
  const factor = sortDir.value === "asc" ? 1 : -1;
  return [...filtered].sort((left, right) => {
    if (key === "size") {
      const leftSize = left.path ? (sizes.value[left.path] ?? -1) : -1;
      const rightSize = right.path ? (sizes.value[right.path] ?? -1) : -1;
      return (leftSize - rightSize) * factor;
    }
    const leftValue = (left[key as keyof InstalledPackage] ?? "").toString();
    const rightValue = (right[key as keyof InstalledPackage] ?? "").toString();
    return leftValue.localeCompare(rightValue) * factor;
  });
});

const rows = computed<TableRow[]>(() =>
  visible.value.map((item) => {
    const newer = newerVersion(item);
    return {
      id: `${item.source}:${item.name}`,
      name: item.name,
      version: versionText(item),
      source: item.source,
      kind: item.kind ?? "",
      size: formatSize(item.path ? sizes.value[item.path] : undefined),
      newer,
      package: item,
    };
  }),
);

const totalSize = computed(() => {
  const known = Object.values(sizes.value);
  if (known.length === 0) return null;
  return known.reduce((sum, value) => sum + value, 0);
});

/** 数据有多旧。刚采完不显示，避免每进一次页面都挂一句多余的说明 */
const freshness = computed(() => {
  const at = collectedAt.value;
  if (!at || !loaded.value) return null;
  const age = Date.now() - at;
  if (age < 60_000) return null;
  if (age < 60 * 60_000) return `${Math.floor(age / 60_000)} 分钟前的数据`;
  if (age < 24 * 60 * 60_000) return `${Math.floor(age / (60 * 60_000))} 小时前的数据`;
  return `${Math.floor(age / (24 * 60 * 60_000))} 天前的数据`;
});

/** 空状态分三种：首次采集、确实没有、筛选没匹配到 */
const emptyState = computed(() => {
  if (packages.value.length === 0 && (loading.value || refreshing.value)) {
    return { icon: undefined, title: "正在采集…", description: copy.value.loading };
  }
  if (packages.value.length === 0) {
    return { icon: PhPackage, title: copy.value.emptyTitle, description: copy.value.emptyHint };
  }
  return { icon: PhPackage, title: "没有匹配的包", description: copy.value.filterHint };
});

/** 体积增量测量：只补没量过、或超过 TTL 的路径 */
async function measure(force = false) {
  const now = Date.now();
  const paths = [
    ...new Set(
      packages.value.map((item) => item.path).filter((path): path is string => Boolean(path)),
    ),
  ];
  const stale = paths.filter(
    (path) => force || sizesAt.value[path] === undefined || now - sizesAt.value[path] > SIZE_TTL_MS,
  );
  if (stale.length === 0) return;

  measuring.value = true;
  try {
    const entries = await invoke<SizeEntry[]>("measure_package_sizes", { paths: stale });
    const nextSizes = { ...sizes.value };
    const nextAt = { ...sizesAt.value };
    for (const entry of entries) {
      nextSizes[entry.path] = entry.bytes;
    }
    // 量不到的路径也记时间，否则每次都会重试同一批无权限目录
    for (const path of stale) {
      nextAt[path] = Date.now();
    }
    sizes.value = nextSizes;
    sizesAt.value = nextAt;
    evictSizes();
    persist();
  } catch {
    // 测体积失败不影响列表本身，保持占位显示
  } finally {
    measuring.value = false;
  }
}

/**
 * 已从列表消失的路径不再保留，避免缓存越滚越大。
 *
 * 单独成函数、由每次采集调用：只把它写在 measure() 里的话，没有过期项时
 * measure() 会提前返回，被卸载的包体积就一直留在 localStorage 里。
 */
function evictSizes() {
  const alive = new Set(
    packages.value.map((item) => item.path).filter((path): path is string => Boolean(path)),
  );
  sizes.value = Object.fromEntries(
    Object.entries(sizes.value).filter(([path]) => alive.has(path)),
  );
  sizesAt.value = Object.fromEntries(
    Object.entries(sizesAt.value).filter(([path]) => alive.has(path)),
  );
}

function persist() {
  writeInstalledCache(props.scope, {
    at: collectedAt.value ?? Date.now(),
    packages: packages.value,
    errors: errors.value,
    sizes: sizes.value,
    sizesAt: sizesAt.value,
    latest: latestByName.value,
    newer: newerByName.value,
  });
}

async function fetchLatestVersions() {
  const targets = packages.value.filter(
    (item) => item.source !== "brew" && item.source !== "deno" && item.version,
  );
  const names = [...new Set(targets.map((item) => item.name))];
  if (names.length === 0) return;
  checkingLatest.value = true;
  try {
    const results = await invoke<LatestVersion[]>("latest_versions", {
      names,
      ...requestOptions(),
    });
    const map: Record<string, string> = {};
    for (const item of results) {
      if (item.latest) map[item.name] = item.latest;
    }
    latestByName.value = map;

    // 版本比较放后端：前端自己比会漏掉 canary / 预发布这些情况
    const local = new Map(targets.map((item) => [item.name, item.version]));
    const probes: VersionProbe[] = Object.entries(map)
      .map(([name, latest]) => ({ name, local: local.get(name) ?? "", latest }))
      .filter((probe) => probe.local !== "");
    if (probes.length > 0) {
      const verdicts = await invoke<VersionVerdict[]>("newer_versions", { probes });
      newerByName.value = Object.fromEntries(
        verdicts.map((item) => [item.name, item.newer]),
      );
    }
    persist();
  } catch {
    // 只是少了升级提示，不影响列表本身
  } finally {
    checkingLatest.value = false;
  }
}

/**
 * 采集。`background` 表示屏幕上已经有上次的数据，这次只是更新——此时不清空列表、
 * 不收起详情，按钮旁边显示"更新中"而不是整页 loading。
 */
async function load(options: { background?: boolean; force?: boolean } = {}) {
  if (!isTauri) return;
  const { background = false, force = false } = options;
  if (background) {
    refreshing.value = true;
  } else {
    loading.value = true;
    selected.value = null;
  }
  loadError.value = null;
  try {
    const report = await invoke<InstalledReport>("list_installed_packages", {
      sources: SOURCES[props.scope],
    });
    // 版本变了的包（升级 / 重装）体积要重新量，没变的沿用缓存
    const before = new Map(
      packages.value.map((item) => [`${item.source}:${item.name}`, item.version]),
    );
    for (const item of report.packages) {
      const previous = before.get(`${item.source}:${item.name}`);
      if (item.path && previous !== undefined && previous !== item.version) {
        delete sizes.value[item.path];
        delete sizesAt.value[item.path];
      }
    }

    packages.value = report.packages;
    errors.value = report.errors;
    collectedAt.value = Date.now();
    loaded.value = true;
    // 卸载掉的包体积要跟着清掉，和有没有过期项无关
    evictSizes();
    persist();

    // 详情面板指向刷新后的那条记录，避免拿着旧对象
    if (selected.value) {
      selected.value =
        report.packages.find(
          (item) => item.name === selected.value?.name && item.source === selected.value?.source,
        ) ?? null;
    }

    void measure(force);
    void fetchLatestVersions();
  } catch (error) {
    loadError.value = error instanceof Error ? error.message : String(error);
  } finally {
    loading.value = false;
    refreshing.value = false;
  }
}

function toggleSort(key: string) {
  if (sortKey.value === key) {
    sortDir.value = sortDir.value === "asc" ? "desc" : "asc";
  } else {
    sortKey.value = key;
    sortDir.value = "asc";
  }
}

function select(row: TableRow) {
  selected.value = row.package as InstalledPackage;
}

watch(selected, () => {
  installedAction.value = "upgrade";
  openError.value = null;
});

async function onActionFinished(code: number | null) {
  if (code !== 0) return;
  // 给包管理器一点落盘时间再重扫，否则可能读到旧状态
  await new Promise((resolve) => setTimeout(resolve, 800));
  // 后台更新：保留当前列表与选中项，避免详情面板突然收起
  await load({ background: true });
}

/** 打开 cask 应用 / 在访达中显示。路径来自扫描结果，Rust 侧仍会做存在性校验。 */
async function openTarget(mode: "open" | "reveal") {
  const path = selected.value?.appPath;
  if (!path) return;
  opening.value = mode;
  openError.value = null;
  try {
    await invoke<string>("open_local_target", { path, mode });
  } catch (error) {
    openError.value = error instanceof Error ? error.message : String(error);
  } finally {
    opening.value = null;
  }
}

onMounted(() => {
  // 有缓存就先渲染旧数据再后台更新；没有缓存才显示"正在采集"
  void load({ background: loaded.value });
});
</script>

<template>
  <div class="grid grid-cols-[minmax(0,1fr)_22rem] items-start gap-3">
    <div class="flex min-w-0 flex-col gap-3">
    <header class="flex items-end justify-between gap-4">
      <p class="text-muted-foreground">{{ copy.subtitle }}</p>
      <div class="ml-auto flex shrink-0 items-center gap-3">
        <StatusBadge v-if="loading" tone="busy" label="采集" />
        <StatusBadge v-else-if="refreshing" tone="busy" label="更新中" />
        <StatusBadge v-else-if="measuring" tone="busy" label="测体积" />
        <StatusBadge v-else-if="checkingLatest" tone="busy" label="查最新版本" />
        <p class="font-mono text-label text-muted-foreground">
          {{ visible.length }} / {{ packages.length }}
          <span v-if="totalSize !== null">· {{ formatSize(totalSize) }}</span>
          <span v-if="upgradableCount > 0">· 可升级 {{ upgradableCount }}</span>
        </p>
        <span v-if="freshness" class="text-caption text-muted-foreground">{{ freshness }}</span>
        <!-- 刷新是显式动作：连体积一起重量，不沿用缓存 -->
        <AppButton :disabled="loading || refreshing || !isTauri" @click="load({ force: true })">
          <PhArrowClockwise :size="14" aria-hidden="true" />
          刷新
        </AppButton>
      </div>
    </header>

    <p v-if="!isTauri" class="rounded-card border border-danger px-3 py-2 text-danger" role="alert">
      当前不在 Tauri 中运行，拿不到本机数据。请用 <code class="font-mono">pnpm tauri:dev</code> 启动。
    </p>

    <p v-if="loadError" class="rounded-card border border-danger px-3 py-2 text-danger" role="alert">
      采集失败：{{ loadError }}
    </p>

    <!-- 单个来源失败要如实说明，不能装作只采到这些包 -->
    <ul v-if="errors.length > 0" class="flex flex-col gap-1">
      <li
        v-for="item in errors"
        :key="item.source"
        class="flex items-start gap-2 rounded-card border border-border bg-surface px-3 py-2"
      >
        <PhWarningCircle :size="15" class="mt-0.5 shrink-0 text-muted-foreground" aria-hidden="true" />
        <span class="wrap-token">
          <span class="font-mono text-label">{{ item.source }}</span>
          未能采集：{{ item.message }}
        </span>
      </li>
    </ul>

    <div class="flex flex-wrap items-center gap-2">
      <div class="min-w-64 flex-1">
        <AppSearchField
          v-model="query"
          label="搜索已安装的包"
          placeholder="按包名或说明过滤…"
          :show-button="false"
        />
      </div>
      <!-- scope="brew" 时只有一个来源，再给一排「全部 / brew」筛选没有意义 -->
      <div v-if="showSourceFilter" class="flex flex-wrap gap-1.5">
        <AppTag
          v-for="item in sourceFilters"
          :key="item.name"
          :label="item.name"
          :count="item.count"
          :active="source === item.name"
          @click="source = item.name"
        />
      </div>
    </div>

    <div class="overflow-hidden rounded-card border border-border bg-surface">
      <AppTable
        :caption="copy.caption"
        :columns="columns"
        :rows="rows"
        :sort-key="sortKey"
        :sort-dir="sortDir"
        @sort="toggleSort"
      >
        <template #cell="{ row, column }">
          <button
            v-if="column.key === 'name'"
            type="button"
            class="cursor-pointer text-left font-mono transition-colors duration-150 hover:text-accent"
            :class="selected?.name === row.name && selected?.source === row.source ? 'text-accent' : ''"
            @click="select(row)"
          >
            {{ row.name }}
          </button>
          <template v-else-if="column.key === 'version'">
            <span>{{ row.version }}</span>
            <span
              v-if="row.newer"
              class="ml-1 text-accent"
              :title="`registry 上已有 ${row.newer}`"
            >↑</span>
          </template>
          <template v-else>{{ row[column.key] }}</template>
        </template>

        <template #empty>
          <AppEmptyState
            :icon="emptyState.icon"
            :title="emptyState.title"
            :description="emptyState.description"
          />
        </template>
      </AppTable>
    </div>
    </div>

    <!--
      只做 sticky，不加 max-h + overflow：那会造出第二根滚动条（main 一层、这里一层）。
      技能也明确要求避免嵌套滚动区域。
    -->
    <div class="sticky top-4">
    <AppDetailPane
      title="包详情"
      :empty="selected === null"
      :icon="PhPackage"
      empty-title="未选中任何包"
      empty-description="点列表里的包名查看版本、安装路径与说明。"
    >
      <template #actions>
        <StatusBadge v-if="selected && newerVersion(selected)" tone="warn" label="可升级" />
        <StatusBadge v-else-if="selected?.revisionOutdated" tone="neutral" label="配方已更新" />
      </template>

      <dl v-if="selected" class="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2">
        <dt class="text-muted-foreground">包名</dt>
        <dd class="wrap-token font-mono">{{ selected.name }}</dd>

        <dt class="text-muted-foreground">版本</dt>
        <dd class="font-mono">
          {{ versionText(selected) }}
          <span v-if="newerVersion(selected)" class="text-muted-foreground">
            → {{ newerVersion(selected) }}
          </span>
        </dd>

        <dt class="text-muted-foreground">来源</dt>
        <dd class="font-mono">
          {{ selected.source }}<span v-if="selected.kind"> · {{ selected.kind }}</span>
        </dd>

        <dt class="text-muted-foreground">安装路径</dt>
        <dd class="wrap-token font-mono">{{ selected.path ?? "—" }}</dd>

        <dt class="text-muted-foreground">磁盘占用</dt>
        <dd class="font-mono">
          {{ formatSize(selected.path ? sizes[selected.path] : undefined) }}
        </dd>

        <template v-if="selected.description">
          <dt class="text-muted-foreground">说明</dt>
          <dd class="wrap-token">{{ selected.description }}</dd>
        </template>

        <template v-if="selected.homepage">
          <dt class="text-muted-foreground">主页</dt>
          <dd class="wrap-token font-mono text-accent">{{ selected.homepage }}</dd>
        </template>
      </dl>

      <!-- cask 是 GUI 应用，装完就该能直接打开，而不是让人自己去 /Applications 里找 -->
      <div v-if="selected?.appPath" class="mt-3 border-t border-border pt-3">
        <p class="label-mini mb-2">应用</p>
        <div class="mb-2 flex flex-wrap gap-1.5">
          <AppButton size="sm" :loading="opening === 'open'" @click="openTarget('open')">
            <PhArrowSquareOut :size="13" aria-hidden="true" />
            打开
          </AppButton>
          <AppButton size="sm" :loading="opening === 'reveal'" @click="openTarget('reveal')">
            <PhFolderOpen :size="13" aria-hidden="true" />
            在访达中显示
          </AppButton>
        </div>
        <p class="wrap-token font-mono text-caption text-muted-foreground">{{ selected.appPath }}</p>
        <p v-if="openError" class="mt-2 wrap-token text-danger" role="alert">{{ openError }}</p>
      </div>

      <p
        v-if="selected?.revisionOutdated"
        class="mt-3 wrap-token rounded-control border border-border bg-muted px-3 py-2 text-label text-muted-foreground"
      >
        Homebrew 标记为过期，但版本号没有变化：是 formula 的修订号更新了，
        「升级」相当于按新配方重建同一版本。
      </p>

      <div v-if="selected" class="mt-3 border-t border-border pt-3">
        <p class="label-mini mb-2">操作</p>
        <div class="mb-3 flex flex-wrap gap-1.5">
          <AppTag
            label="升级到最新"
            :active="installedAction === 'upgrade'"
            @click="installedAction = 'upgrade'"
          />
          <AppTag
            label="卸载"
            :active="installedAction === 'uninstall'"
            @click="installedAction = 'uninstall'"
          />
        </div>
        <ActionPanel
          :key="`${selected.source}:${selected.name}:${installedAction}`"
          :source="selected.source"
          :action="installedAction"
          :name="selected.name"
          :version="upgradeVersion"
          :kind="selected.kind"
          :upgrade-available="newerVersion(selected) !== null"
          @finished="onActionFinished"
        />
      </div>
    </AppDetailPane>
    </div>
  </div>
</template>
