<script setup lang="ts">
// 市场：真实搜索 + 详情 + README 按需加载。
//
// 布局取向与 npm 官网不同：结果用列表而不是表格，因为 npm 的描述普遍很长，
// 塞进表格单元格会被压成一团。README 不随详情一起取——实测完整 packument
// 有 6.8MB~15MB，改为按需从 jsDelivr 拉单个 README.md。
import { computed, onMounted, onUnmounted, ref } from "vue";
import { PhArrowClockwise, PhMagnifyingGlass, PhWarningCircle } from "@phosphor-icons/vue";
import { invoke } from "@tauri-apps/api/core";

import AppButton from "../components/ui/AppButton.vue";
import AppDetailPane from "../components/ui/AppDetailPane.vue";
import AppExternalLink from "../components/ui/AppExternalLink.vue";
import AppEmptyState from "../components/ui/AppEmptyState.vue";
import AppSearchField from "../components/ui/AppSearchField.vue";
import AppSparkline from "../components/ui/AppSparkline.vue";
import AppTag from "../components/ui/AppTag.vue";
import StatusBadge from "../components/ui/StatusBadge.vue";
import ActionPanel from "../components/ActionPanel.vue";
import { isTauri } from "../lib/tauri";
import { formatNumber } from "../lib/format";
import { useScrollLock } from "../composables/useScrollLock";
import { useSettings } from "../composables/useSettings";
import type {
  PackageDetail,
  PackageVersion,
  ReadmeResponse,
  SearchHit,
  SearchResponse,
  VersionHistory,
} from "../types";

const PAGE_SIZE = 20;
/** 版本历史先显示最近这些，其余点「显示全部」再展开 */
const VERSION_PREVIEW = 8;
const MANAGERS = ["npm", "pnpm", "bun", "deno"];

// 数据源与代理来自设置页，每次请求现取现用，改完保存即刻生效
const { requestOptions } = useSettings();

const ECOSYSTEM = ["vue", "react", "vite", "vitest", "next", "astro", "svelte", "typescript", "eslint", "biome", "tailwindcss", "pnpm"];

const query = ref("");
const submitted = ref("");
const hits = ref<SearchHit[]>([]);
const total = ref(0);
const loading = ref(false);
const loadingMore = ref(false);
const searchError = ref<string | null>(null);
const tookMs = ref<number | null>(null);
const fromCache = ref(false);

const selected = ref<SearchHit | null>(null);
const detail = ref<PackageDetail | null>(null);
const detailLoading = ref(false);
const detailError = ref<string | null>(null);
/** 主页 / 仓库外链没打开成功时的说明 */
const openError = ref<string | null>(null);

const readme = ref<ReadmeResponse | null>(null);
const readmeLoading = ref(false);
const readmeError = ref<string | null>(null);
const readmeOpen = ref(false);
const showAllDependencies = ref(false);

const history = ref<VersionHistory | null>(null);
const historyLoading = ref(false);
const historyError = ref<string | null>(null);
const showAllVersions = ref(false);

const managerAvailable = ref<Record<string, boolean>>({});
const installSource = ref("npm");

let debounce: number | undefined;
// 每次请求带序号，避免慢响应覆盖新结果（技能：快速变更要安全取消/替换）
let requestId = 0;

const hasMore = computed(() => hits.value.length > 0 && hits.value.length < total.value);
const visibleDependencies = computed(() => {
  const list = detail.value?.dependencies ?? [];
  return showAllDependencies.value ? list : list.slice(0, 8);
});
/** 版本总数：优先用版本历史里的权威值，加载前退回到搜索结果里的版本数组长度 */
const versionCount = computed(() => history.value?.total ?? selected.value?.versionCount ?? null);
const visibleVersions = computed(() => {
  const list = history.value?.versions ?? [];
  return showAllVersions.value ? list : list.slice(0, VERSION_PREVIEW);
});
const seriesLabel = computed(() => {
  const series = detail.value?.weeklySeries ?? [];
  if (series.length < 2) return "";
  return `近 7 天下载趋势，最低 ${formatNumber(Math.min(...series))}，最高 ${formatNumber(Math.max(...series))}`;
});

function formatBytes(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  if (value >= 1024 * 1024) return `${(value / 1024 / 1024).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(value / 1024))} KB`;
}

function formatDate(value: string | null | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toISOString().slice(0, 10);
}

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** 相对时间：发布时间看「多久前」比看日期更能判断新鲜度，超过一个月才退回日期 */
function formatRelative(value: string | null | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  const elapsed = Date.now() - date.getTime();
  if (elapsed < MINUTE) return "刚刚";
  if (elapsed < HOUR) return `${Math.floor(elapsed / MINUTE)} 分钟前`;
  if (elapsed < DAY) return `${Math.floor(elapsed / HOUR)} 小时前`;
  if (elapsed < 30 * DAY) return `${Math.floor(elapsed / DAY)} 天前`;
  return formatDate(value);
}

/** 官方接口不列出「近一周零下载」的版本，所以缺失就是 0；整体取不到才显示占位符 */
function versionDownloads(item: PackageVersion): string {
  if (!history.value?.downloadsSource || history.value.downloadsError) return "—";
  return formatNumber(item.downloadsLastWeek ?? 0);
}

function onInput() {
  window.clearTimeout(debounce);
  debounce = window.setTimeout(() => void search(false), 300);
}

async function search(more: boolean) {
  const keyword = query.value.trim();
  if (!keyword || !isTauri) return;
  if (!more && keyword === submitted.value && hits.value.length > 0) return;

  const id = ++requestId;
  if (more) {
    loadingMore.value = true;
  } else {
    loading.value = true;
    searchError.value = null;
    hits.value = [];
    total.value = 0;
    selected.value = null;
    detail.value = null;
    readme.value = null;
  }

  try {
    const response = await invoke<SearchResponse>("search_packages", {
      query: keyword,
      size: PAGE_SIZE,
      from: more ? hits.value.length : 0,
      ...requestOptions(),
    });
    if (id !== requestId) return; // 已有更新的请求，丢弃这次结果
    hits.value = more ? [...hits.value, ...response.hits] : response.hits;
    total.value = response.total;
    submitted.value = keyword;
    tookMs.value = response.tookMs;
    fromCache.value = response.cached;
  } catch (error) {
    if (id !== requestId) return;
    searchError.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (id === requestId) {
      loading.value = false;
      loadingMore.value = false;
    }
  }
}

/** 主页 / 仓库外链没打开成功时如实说明 */
function onLinkFailed(message: string) {
  openError.value = message;
}

async function select(hit: SearchHit) {
  selected.value = hit;
  detail.value = null;
  detailError.value = null;
  openError.value = null;
  readme.value = null;
  readmeError.value = null;
  history.value = null;
  historyError.value = null;
  // 上一个包的请求可能还在飞，它的 finally 不会再动这个开关，这里显式复位
  historyLoading.value = false;
  showAllDependencies.value = false;
  showAllVersions.value = false;
  detailLoading.value = true;
  const name = hit.name;
  try {
    const result = await invoke<PackageDetail>("package_detail", {
      name,
      ...requestOptions(),
    });
    // 慢响应不能覆盖后点的包
    if (selected.value?.name !== name) return;
    detail.value = result;
  } catch (error) {
    if (selected.value?.name !== name) return;
    detailError.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (selected.value?.name === name) detailLoading.value = false;
  }
}

/** 版本历史按需加载：完整 packument 即使 gzip 后仍可能上兆 */
async function loadVersions() {
  const name = selected.value?.name;
  if (!name || historyLoading.value) return;
  historyLoading.value = true;
  historyError.value = null;
  try {
    const result = await invoke<VersionHistory>("package_versions", {
      name,
      ...requestOptions(),
    });
    if (selected.value?.name !== name) return;
    history.value = result;
    showAllVersions.value = false;
  } catch (error) {
    if (selected.value?.name !== name) return;
    historyError.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (selected.value?.name === name) historyLoading.value = false;
  }
}

async function loadReadme() {
  if (!selected.value) return;
  // 与详情、版本历史一样要守卫：A 的 README 慢返回不能落到已经切到 B 的界面上
  const name = selected.value.name;
  const version = detail.value?.version ?? selected.value.version;
  readmeLoading.value = true;
  readmeError.value = null;
  try {
    const options = requestOptions();
    const result = await invoke<ReadmeResponse>("package_readme", {
      name,
      version,
      proxy: options.proxy,
      insecure: options.insecure,
    });
    if (selected.value?.name !== name) return;
    readme.value = result;
  } catch (error) {
    if (selected.value?.name !== name) return;
    readmeError.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (selected.value?.name === name) readmeLoading.value = false;
  }
}

function quickSearch(name: string) {
  query.value = name;
  void search(false);
}

function onWindowKey(event: KeyboardEvent) {
  if (readmeOpen.value && event.key === "Escape") readmeOpen.value = false;
}

onMounted(async () => {
  if (!isTauri) return;
  try {
    const entries = await Promise.all(
      MANAGERS.map(async (manager) => {
        const path = await invoke<string | null>("resolve_program", { program: manager });
        return [manager, Boolean(path)] as const;
      }),
    );
    managerAvailable.value = Object.fromEntries(entries);
    const firstAvailable = MANAGERS.find((manager) => managerAvailable.value[manager]);
    if (firstAvailable) installSource.value = firstAvailable;
  } catch {
    // 探测失败不该让整个视图挂掉：标签全部置灰，用户仍能搜索与查看详情
    managerAvailable.value = Object.fromEntries(MANAGERS.map((manager) => [manager, false]));
  }
});

onUnmounted(() => window.clearTimeout(debounce));
onMounted(() => window.addEventListener("keydown", onWindowKey));
onUnmounted(() => window.removeEventListener("keydown", onWindowKey));
useScrollLock(readmeOpen);
</script>

<template>
  <div class="flex flex-col gap-3">
    <AppSearchField
      v-model="query"
      label="搜索 npm 包"
      placeholder="包名，例如 vite"
      size="lg"
      @update:model-value="onInput"
      @submit="search(false)"
    />

    <p v-if="!isTauri" class="rounded-card border border-danger px-3 py-2 text-danger" role="alert">
      当前不在 Tauri 中运行，无法访问包市场。请用 <code class="font-mono">pnpm tauri:dev</code> 启动。
    </p>

    <ul class="flex flex-wrap justify-center gap-x-4 gap-y-2">
      <li v-for="name in ECOSYSTEM" :key="name">
        <AppTag :label="name" dot :active="submitted === name" @click="quickSearch(name)" />
      </li>
    </ul>

    <div class="flex items-center justify-between gap-3">
      <p class="font-mono text-label text-muted-foreground">
        <template v-if="loading">搜索中…</template>
        <template v-else-if="submitted">
          匹配 {{ formatNumber(total) }} 个 · 已显示 {{ hits.length }}
          <span v-if="tookMs !== null"> · {{ tookMs }}ms</span>
          <!-- 后端有 10 分钟缓存，命中时说清楚，免得"怎么这么快"变成疑问 -->
          <span v-if="fromCache"> · 来自缓存</span>
        </template>
        <template v-else>输入关键词开始搜索</template>
      </p>
      <StatusBadge v-if="loading || loadingMore" tone="busy" label="请求中" />
    </div>

    <p v-if="searchError" class="flex items-center gap-2 rounded-card border border-danger px-3 py-2" role="alert">
      <PhWarningCircle :size="15" class="shrink-0 text-danger" aria-hidden="true" />
      <span class="min-w-0 flex-1 wrap-token text-danger">搜索失败：{{ searchError }}</span>
      <AppButton size="sm" @click="search(false)">
        <PhArrowClockwise :size="13" aria-hidden="true" />
        重试
      </AppButton>
    </p>

    <div class="grid grid-cols-[minmax(0,1fr)_22rem] items-start gap-3">
    <div class="flex min-w-0 flex-col gap-3">
    <ul v-if="hits.length > 0" class="flex flex-col overflow-hidden rounded-card border border-border bg-surface">
      <li v-for="hit in hits" :key="hit.name" class="border-b border-border last:border-b-0">
        <button
          type="button"
          class="flex w-full cursor-pointer flex-col gap-1 px-3 py-2.5 text-left transition-colors duration-150 hover:bg-muted"
          :class="selected?.name === hit.name ? 'border-l-2 border-l-accent bg-muted' : 'border-l-2 border-l-transparent'"
          @click="select(hit)"
        >
          <span class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
            <span class="font-mono text-body font-medium">{{ hit.name }}</span>
            <span class="font-mono text-caption text-muted-foreground">{{ hit.version }}</span>
            <span v-if="hit.publisher" class="font-mono text-caption text-muted-foreground">{{ hit.publisher }}</span>
            <span class="font-mono text-caption text-muted-foreground">{{ formatDate(hit.publishedAt) }}</span>
          </span>
          <span v-if="hit.description" class="wrap-token text-muted-foreground">{{ hit.description }}</span>
        </button>
      </li>
    </ul>

    <AppButton v-if="hasMore" :loading="loadingMore" @click="search(true)">加载更多</AppButton>

    <div v-if="hits.length === 0 && !loading && !searchError" class="rounded-card border border-border bg-surface">
      <AppEmptyState
        :icon="PhMagnifyingGlass"
        :title="submitted ? `没有匹配「${submitted}」的包` : '搜索 npm 包'"
        :description="
          submitted
            ? '换个关键词，或点上方的生态标签快速搜索。'
            : '默认走 npmmirror 源。选中结果后可以查看版本、依赖与 README。'
        "
      />
    </div>
    </div>

    <!-- 同已安装页：只 sticky，不建第二个滚动容器 -->
    <div class="sticky top-4">
    <AppDetailPane
      title="包详情"
      :empty="selected === null"
      :icon="PhMagnifyingGlass"
      empty-title="未选中任何包"
      empty-description="在上面的结果里点一个包，这里会显示当前标签、版本历史、下载量趋势与安装入口。"
    >
      <template #actions>
        <StatusBadge v-if="detailLoading" tone="busy" label="读取中" />
        <StatusBadge v-else-if="detail?.deprecated" tone="warn" label="已废弃" />
      </template>

      <p v-if="detailError" class="rounded-card border border-danger px-3 py-2 text-danger" role="alert">
        详情读取失败：{{ detailError }}
      </p>

      <div v-else-if="selected" class="flex flex-col gap-3">
        <p v-if="detail?.deprecated" class="wrap-token rounded-card border border-danger px-3 py-2 text-danger">
          {{ detail.deprecated }}
        </p>

        <p class="wrap-token">{{ detail?.description ?? selected.description ?? "暂无说明" }}</p>

        <dl class="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2">
          <dt class="text-muted-foreground">版本</dt>
          <dd class="font-mono">{{ detail?.version ?? selected.version }}</dd>

          <dt class="text-muted-foreground">标签</dt>
          <dd v-if="detail?.distTags.length" class="flex flex-wrap items-center gap-1">
            <AppTag
              v-for="item in detail.distTags"
              :key="item.tag"
              :label="`${item.tag} ${item.version}`"
            />
          </dd>
          <dd v-else class="text-muted-foreground">—</dd>

          <dt class="text-muted-foreground">许可证</dt>
          <dd class="font-mono">{{ detail?.license ?? "—" }}</dd>

          <dt class="text-muted-foreground">周下载量</dt>
          <dd class="flex flex-wrap items-center gap-2">
            <span class="font-mono tabular-nums">{{ formatNumber(detail?.weeklyDownloads) }}</span>
            <AppSparkline :values="detail?.weeklySeries ?? []" :label="seriesLabel" />
            <span
              v-if="detail?.downloadsSource === 'npmmirror'"
              class="text-caption text-muted-foreground"
              title="官方接口不可用，退回镜像站统计，数值只含镜像流量"
            >镜像口径</span>
            <span
              v-if="detail?.downloadsError"
              class="text-caption text-muted-foreground"
              :title="detail.downloadsError"
            >（取不到）</span>
          </dd>

          <dt class="text-muted-foreground">解压体积</dt>
          <dd class="font-mono">
            {{ formatBytes(detail?.unpackedSize) }}
            <span v-if="detail?.fileCount" class="text-muted-foreground">· {{ detail.fileCount }} 个文件</span>
          </dd>

          <dt class="text-muted-foreground">发布时间</dt>
          <dd class="font-mono">{{ formatDate(selected.publishedAt) }}</dd>

          <template v-if="detail?.homepage">
            <dt class="text-muted-foreground">主页</dt>
            <dd>
              <AppExternalLink
                :url="detail.homepage"
                :window-title="`${selected.name} · 主页`"
                @failed="onLinkFailed"
              />
            </dd>
          </template>

          <template v-if="detail?.repository">
            <dt class="text-muted-foreground">仓库</dt>
            <dd>
              <AppExternalLink
                :url="detail.repository"
                :window-title="`${selected.name} · 仓库`"
                @failed="onLinkFailed"
              />
            </dd>
          </template>
        </dl>

        <p v-if="openError" class="mt-3 wrap-token text-danger" role="alert">{{ openError }}</p>

        <!-- 版本历史与 npm 官网对齐：版本数、当前标签、每个版本的发布时间与近一周下载量 -->
        <div class="border-t border-border pt-3">
          <div class="mb-2 flex items-center justify-between gap-2">
            <p class="label-mini">
              版本历史<template v-if="versionCount !== null"> · 共 {{ formatNumber(versionCount) }} 个</template>
            </p>
            <AppButton
              v-if="!history && !historyError"
              size="sm"
              :loading="historyLoading"
              @click="loadVersions"
            >查看</AppButton>
            <AppButton
              v-else-if="history && history.versions.length > VERSION_PREVIEW"
              size="sm"
              @click="showAllVersions = !showAllVersions"
            >
              {{ showAllVersions ? "只看最新" : "显示全部" }}
            </AppButton>
          </div>

          <p v-if="historyError" class="flex items-center gap-2" role="alert">
            <span class="min-w-0 flex-1 wrap-token text-danger">版本列表读取失败：{{ historyError }}</span>
            <AppButton size="sm" @click="loadVersions">重试</AppButton>
          </p>
          <p v-else-if="historyLoading" class="text-caption text-muted-foreground">
            正在拉取完整版本清单，大包（如上万个版本）需要几秒…
          </p>
          <p v-else-if="history" class="text-caption text-muted-foreground">
            最近发布 {{ formatRelative(history.lastPublishedAt) }}
            <template v-if="history.truncated"> · 只列出最近 {{ formatNumber(history.versions.length) }} 个版本</template>
            <template v-if="history.downloadsError"> · {{ history.downloadsError }}</template>
          </p>

          <!-- 用真正的 table 而不是并排的 grid：表头与数据行各自成格时列宽会各算各的，
               实测「7 日下载」表头会和内容列错开一整个版本列宽 -->
          <table v-if="history && history.versions.length" class="mt-2 w-full table-fixed text-caption">
            <colgroup>
              <col />
              <col class="w-11" />
              <col class="w-14" />
              <col class="w-13" />
            </colgroup>
            <thead>
              <tr class="border-b border-border label-mini">
                <th scope="col" class="pb-1 text-left font-normal">版本</th>
                <th scope="col" class="pb-1 text-left font-normal">标签</th>
                <th scope="col" class="pb-1 text-right font-normal">7 日下载</th>
                <th scope="col" class="pb-1 text-right font-normal">发布时间</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="item in visibleVersions"
                :key="item.version"
                class="border-b border-border last:border-b-0"
              >
                <td class="max-w-0 truncate py-1 pr-2 font-mono" :title="item.version">
                  {{ item.version }}
                </td>
                <td class="py-1 pr-2">
                  <span v-if="item.deprecated" class="whitespace-nowrap">
                    <PhWarningCircle :size="11" class="inline" aria-hidden="true" />
                    已废弃
                  </span>
                  <span v-else class="font-mono text-muted-foreground">{{ item.tags[0] ?? "" }}</span>
                </td>
                <td class="py-1 pr-2 text-right font-mono tabular-nums">
                  {{ versionDownloads(item) }}
                </td>
                <td class="py-1 text-right text-muted-foreground">
                  {{ formatRelative(item.publishedAt) }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <div v-if="detail && detail.dependencies.length > 0" class="border-t border-border pt-3">
          <div class="mb-2 flex items-center justify-between gap-2">
            <p class="label-mini">依赖 {{ detail.dependencies.length }} 个</p>
            <AppButton
              v-if="detail.dependencies.length > 8"
              size="sm"
              @click="showAllDependencies = !showAllDependencies"
            >
              {{ showAllDependencies ? "收起" : `展开全部` }}
            </AppButton>
          </div>
          <!-- 单列展示：响应式多列在 dev 环境下未能验证生效，不押注未验证的布局 -->
          <ul class="flex flex-col gap-y-1">
            <li v-for="item in visibleDependencies" :key="item.name" class="flex items-baseline gap-2 font-mono text-label">
              <span class="min-w-0 wrap-token">{{ item.name }}</span>
              <span class="shrink-0 text-muted-foreground">{{ item.range }}</span>
            </li>
          </ul>
          <p v-if="detail.peerDependencies.length > 0" class="mt-2 text-caption text-muted-foreground">
            另有 {{ detail.peerDependencies.length }} 个 peer 依赖
          </p>
        </div>

        <div class="border-t border-border pt-3">
          <p class="label-mini mb-2">安装方式</p>
          <div class="mb-3 flex flex-wrap gap-1.5">
            <AppTag
              v-for="manager in MANAGERS"
              :key="manager"
              :label="manager"
              :active="installSource === manager"
              :disabled="!managerAvailable[manager]"
              :title="managerAvailable[manager] ? undefined : `${manager} 未安装或不在 PATH 中`"
              @click="managerAvailable[manager] && (installSource = manager)"
            />
          </div>
          <ActionPanel
            :source="installSource"
            action="install"
            :name="selected.name"
            :version="detail?.version ?? selected.version"
          />
        </div>

        <div class="border-t border-border pt-3">
          <div class="mb-2 flex items-center justify-between gap-2">
            <p class="label-mini">说明文档</p>
            <AppButton v-if="!readme" :loading="readmeLoading" @click="loadReadme">加载 README</AppButton>
            <AppButton v-else @click="readmeOpen = true">查看 README</AppButton>
          </div>

          <p v-if="readmeError" class="text-danger" role="alert">README 读取失败：{{ readmeError }}</p>
          <p v-else class="text-caption text-muted-foreground">
            README 不随详情自动加载，按需从 jsDelivr 取单个 README.md；内容在弹层里查看，
            避免侧栏出现第二根滚动条。
          </p>
        </div>

      </div>
    </AppDetailPane>
    </div>
    </div>

    <!-- README 用模态承载：长内容自带滚动是合理的，放在侧栏里则会和页面滚动打架 -->
    <div
      v-if="readmeOpen && readme"
      class="fixed inset-0 z-1000 flex items-center justify-center bg-foreground/20 p-6 backdrop-blur-sm"
      @click.self="readmeOpen = false"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="readme-title"
        class="flex max-h-[80vh] w-full max-w-3xl flex-col overflow-hidden rounded-card border border-border bg-surface"
      >
        <header class="flex items-center justify-between gap-3 border-b border-border px-4 py-2.5">
          <h3 id="readme-title" class="min-w-0 truncate font-mono text-title font-medium">
            {{ readme.name }}@{{ readme.version }} · README
          </h3>
          <div class="flex shrink-0 items-center gap-2">
            <a
              :href="readme.sourceUrl"
              target="_blank"
              rel="noreferrer"
              class="font-mono text-caption text-accent"
            >在浏览器打开</a>
            <AppButton @click="readmeOpen = false">关闭</AppButton>
          </div>
        </header>
        <pre
          class="min-h-0 flex-1 overflow-auto bg-muted p-4 font-mono text-caption leading-relaxed whitespace-pre-wrap wrap-token"
        >{{ readme.markdown }}</pre>
      </div>
    </div>
  </div>
</template>
