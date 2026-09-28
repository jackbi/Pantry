<script setup lang="ts">
// Homebrew 页：搜索 formula / cask → 详情 → 安装、升级、卸载。
//
// 为什么不并进 npm 市场：brew 是本机命令，没有分页、没有 README、没有下载趋势，
// 数据形态与 npmmirror 完全不同；两边塞进一个视图只会让状态互相污染。
//
// 索引策略：所有 brew 调用都带 HOMEBREW_NO_AUTO_UPDATE=1。自动更新一天最多触发一次，
// 但一旦触发就是几十秒白等，所以改成用户在诊断页显式跑「brew 更新索引」。
import { computed, onMounted, onUnmounted, ref } from "vue";
import {
  PhArrowClockwise,
  PhArrowSquareOut,
  PhBeerStein,
  PhFolderOpen,
  PhWarningCircle,
} from "@phosphor-icons/vue";
import { invoke } from "@tauri-apps/api/core";

import AppButton from "../components/ui/AppButton.vue";
import AppDetailPane from "../components/ui/AppDetailPane.vue";
import AppEmptyState from "../components/ui/AppEmptyState.vue";
import AppSearchField from "../components/ui/AppSearchField.vue";
import AppTag from "../components/ui/AppTag.vue";
import StatusBadge from "../components/ui/StatusBadge.vue";
import ActionPanel from "../components/ActionPanel.vue";
import { onActionSettled, type ActionSession } from "../composables/useActionSessions";
import { useSettings } from "../composables/useSettings";
import { isTauri } from "../lib/tauri";
import { formatNumber } from "../lib/format";
import type {
  BrewDetail,
  BrewKind,
  BrewSearchItem,
  BrewSearchResponse,
  BrewStats,
  PackageAction,
} from "../types";

const SUGGESTIONS = ["ripgrep", "jq", "git", "ffmpeg", "docker", "postgresql@17", "visual-studio-code"];

// 只作为 Homebrew 页的「安装新包」视图使用，标题与页边距由外层页面提供
// 安装量取自 formulae.brew.sh，跟随设置页的代理与证书开关
const { requestOptions } = useSettings();

const query = ref("");
const submitted = ref("");
const kindFilter = ref<"all" | BrewKind>("all");
const items = ref<BrewSearchItem[]>([]);
const sourceErrors = ref<string[]>([]);
const tookMs = ref<number | null>(null);
const loading = ref(false);
const searchError = ref<string | null>(null);

const selected = ref<BrewSearchItem | null>(null);
const detail = ref<BrewDetail | null>(null);
const detailLoading = ref(false);
const detailError = ref<string | null>(null);
const stats = ref<BrewStats | null>(null);
const statsLoading = ref(false);
const action = ref<PackageAction>("install");
const notice = ref<string | null>(null);

let debounce: number | undefined;
let requestId = 0;

const visible = computed(() =>
  kindFilter.value === "all"
    ? items.value
    : items.value.filter((item) => item.kind === kindFilter.value),
);

const kindCounts = computed(() => ({
  all: items.value.length,
  formula: items.value.filter((item) => item.kind === "formula").length,
  cask: items.value.filter((item) => item.kind === "cask").length,
}));

const installed = computed(() => Boolean(detail.value?.installed));
const upgradeAvailable = computed(() => detail.value?.latest !== null);

function onInput() {
  window.clearTimeout(debounce);
  debounce = window.setTimeout(() => void search(), 400);
}

async function search() {
  const keyword = query.value.trim();
  if (!keyword || !isTauri) return;
  if (keyword === submitted.value && items.value.length > 0) return;

  const id = ++requestId;
  loading.value = true;
  searchError.value = null;
  selected.value = null;
  detail.value = null;
  stats.value = null;
  try {
    const response = await invoke<BrewSearchResponse>("brew_search", {
      query: keyword,
      proxy: requestOptions().proxy,
    });
    if (id !== requestId) return;
    items.value = response.items;
    sourceErrors.value = response.errors;
    tookMs.value = response.tookMs;
    submitted.value = keyword;
  } catch (error) {
    if (id !== requestId) return;
    searchError.value = error instanceof Error ? error.message : String(error);
    items.value = [];
  } finally {
    if (id === requestId) loading.value = false;
  }
}

async function select(item: BrewSearchItem) {
  selected.value = item;
  detail.value = null;
  detailError.value = null;
  stats.value = null;
  notice.value = null;
  detailLoading.value = true;
  const name = item.name;
  const kind = item.kind;
  try {
    const result = await invoke<BrewDetail>("brew_detail", {
      name,
      kind,
      proxy: requestOptions().proxy,
    });
    if (selected.value?.name !== name) return;
    detail.value = result;
    action.value = result.installed ? "upgrade" : "install";
    // 安装量是第三方统计，单独补；慢也不挡详情
    if (result.kind === "formula") void loadStats(name);
  } catch (error) {
    if (selected.value?.name !== name) return;
    detailError.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (selected.value?.name === name) detailLoading.value = false;
  }
}

async function loadStats(name: string) {
  statsLoading.value = true;
  try {
    // brew 只认代理与证书开关：它的"源"是 tap 仓库，registry 传过去没有意义
    const options = requestOptions();
    const result = await invoke<BrewStats>("brew_stats", {
      name,
      proxy: options.proxy,
      insecure: options.insecure,
    });
    if (selected.value?.name !== name) return;
    stats.value = result;
  } catch (error) {
    if (selected.value?.name !== name) return;
    stats.value = { name, installs30d: null, error: error instanceof Error ? error.message : String(error) };
  } finally {
    if (selected.value?.name === name) statsLoading.value = false;
  }
}

async function openTarget(mode: "open" | "reveal") {
  const path = detail.value?.appPath;
  if (!path) return;
  notice.value = null;
  try {
    await invoke<string>("open_local_target", { path, mode });
  } catch (error) {
    notice.value = error instanceof Error ? error.message : String(error);
  }
}

/**
 * 结束通知来自会话层：面板在切走时就被销毁了，它自己的 emit 发不出来。
 */
onActionSettled((session) => {
  void onActionFinished(session);
});

async function onActionFinished(session: ActionSession) {
  if (session.exitCode !== 0) return;
  const item = selected.value;
  // 订阅是全局的：别的包（或别的页面的 brew 包）执行完不该打断当前详情
  if (!item || session.target.source !== "brew" || item.name !== session.target.name) return;
  // select() 会清空提示，所以要等它读完后再说"已完成"
  await select(item);
  notice.value = "命令已完成，这里是重新读取到的状态";
}

function quickSearch(name: string) {
  query.value = name;
  void search();
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") notice.value = null;
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <AppSearchField
      v-model="query"
      label="搜索 Homebrew 包"
      placeholder="包名，例如 ripgrep"
      size="lg"
      @update:model-value="onInput"
      @submit="search"
    />

    <p v-if="!isTauri" class="rounded-card border border-danger px-3 py-2 text-danger" role="alert">
      当前不在 Tauri 中运行，无法调用 brew。请用 <code class="font-mono">pnpm tauri:dev</code> 启动。
    </p>

    <ul class="flex flex-wrap justify-center gap-x-4 gap-y-2">
      <li v-for="name in SUGGESTIONS" :key="name">
        <AppTag :label="name" dot :active="submitted === name" @click="quickSearch(name)" />
      </li>
    </ul>

    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex items-center gap-1.5">
        <AppTag
          label="全部"
          :count="kindCounts.all"
          :active="kindFilter === 'all'"
          @click="kindFilter = 'all'"
        />
        <AppTag
          label="formula"
          :count="kindCounts.formula"
          :active="kindFilter === 'formula'"
          @click="kindFilter = 'formula'"
        />
        <AppTag
          label="cask"
          :count="kindCounts.cask"
          :active="kindFilter === 'cask'"
          @click="kindFilter = 'cask'"
        />
      </div>
      <p class="font-mono text-label text-muted-foreground">
        <template v-if="loading">搜索中…</template>
        <template v-else-if="submitted">
          匹配 {{ formatNumber(visible.length) }} 个
          <span v-if="tookMs !== null"> · {{ tookMs }}ms</span>
        </template>
        <template v-else>输入关键词开始搜索</template>
      </p>
      <StatusBadge v-if="loading" tone="busy" label="请求中" />
    </div>

    <p v-if="searchError" class="flex items-center gap-2 rounded-card border border-danger px-3 py-2" role="alert">
      <PhWarningCircle :size="15" class="shrink-0 text-danger" aria-hidden="true" />
      <span class="min-w-0 flex-1 wrap-token text-danger">搜索失败：{{ searchError }}</span>
      <AppButton size="sm" @click="search">
        <PhArrowClockwise :size="13" aria-hidden="true" />
        重试
      </AppButton>
    </p>

    <p
      v-for="message in sourceErrors"
      :key="message"
      class="flex items-center gap-2 rounded-card border border-border px-3 py-2"
    >
      <PhWarningCircle :size="15" class="shrink-0 text-muted-foreground" aria-hidden="true" />
      <span class="min-w-0 flex-1 wrap-token text-muted-foreground">{{ message }}</span>
    </p>

    <div class="grid grid-cols-[minmax(0,1fr)_22rem] items-start gap-3">
      <div class="flex min-w-0 flex-col gap-3">
        <ul v-if="visible.length > 0" class="flex flex-col overflow-hidden rounded-card border border-border bg-surface">
          <li v-for="item in visible" :key="`${item.kind}:${item.name}`" class="border-b border-border last:border-b-0">
            <button
              type="button"
              class="flex w-full cursor-pointer flex-col gap-1 px-3 py-2.5 text-left transition-colors duration-150 hover:bg-muted"
              :class="
                selected?.name === item.name
                  ? 'border-l-2 border-l-accent bg-muted'
                  : 'border-l-2 border-l-transparent'
              "
              @click="select(item)"
            >
              <span class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                <span class="font-mono text-body font-medium">{{ item.name }}</span>
                <span class="label-mini">{{ item.kind }}</span>
                <span v-if="item.version" class="font-mono text-caption text-muted-foreground">
                  {{ item.version }}
                </span>
                <StatusBadge v-if="item.installed" tone="ok" label="已安装" />
                <span v-if="item.latest" class="font-mono text-caption text-accent">
                  可升级到 {{ item.latest }}
                </span>
              </span>
              <span v-if="item.description" class="wrap-token text-muted-foreground">
                {{ item.description }}
              </span>
            </button>
          </li>
        </ul>

        <div v-if="visible.length === 0 && !loading && !searchError" class="rounded-card border border-border bg-surface">
          <AppEmptyState
            :icon="PhBeerStein"
            :title="submitted ? `没有匹配「${submitted}」的包` : '搜索 Homebrew 包'"
            :description="
              submitted
                ? '换个关键词，或把类型切回「全部」。brew 搜索基于本机索引，久未更新可以先跑一次 brew update。'
                : '搜索基于本机 brew 索引。选中结果后可查看依赖、冲突与安装量，并直接安装。'
            "
          />
        </div>
      </div>

      <!-- 同其他页：只 sticky，不建第二个滚动容器 -->
      <div class="sticky top-4">
        <AppDetailPane
          title="包详情"
          :empty="selected === null"
          :icon="PhBeerStein"
          empty-title="未选中任何包"
          empty-description="在上面的结果里点一个包，这里会显示版本、依赖、安装量与安装入口。"
        >
          <template #actions>
            <StatusBadge v-if="detailLoading" tone="busy" label="读取中" />
            <StatusBadge v-else-if="detail?.installed && detail.latest" tone="warn" label="可升级" />
            <StatusBadge v-else-if="detail?.installed" tone="ok" label="已安装" />
          </template>

          <p v-if="detailError" class="rounded-card border border-danger px-3 py-2 text-danger" role="alert">
            详情读取失败：{{ detailError }}
          </p>

          <div v-else-if="selected" class="flex flex-col gap-3">
            <p v-if="detail?.deprecation" class="wrap-token rounded-card border border-danger px-3 py-2 text-danger">
              {{ detail.deprecation }}
            </p>

            <p class="wrap-token">
              {{ detail?.description ?? selected.description ?? "暂无说明" }}
            </p>

            <dl class="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2">
              <dt class="text-muted-foreground">类型</dt>
              <dd class="font-mono">{{ detail?.kind ?? selected.kind }}</dd>

              <dt class="text-muted-foreground">最新版本</dt>
              <dd class="font-mono">{{ detail?.version ?? "—" }}</dd>

              <template v-if="detail?.installed">
                <dt class="text-muted-foreground">已安装</dt>
                <dd class="font-mono">
                  {{ detail.installed }}
                  <span v-if="detail.latest" class="text-muted-foreground">→ {{ detail.latest }}</span>
                </dd>
              </template>

              <dt class="text-muted-foreground">30 天安装量</dt>
              <dd class="font-mono tabular-nums">
                <template v-if="detail?.kind === 'cask'">
                  <span class="text-muted-foreground" title="Homebrew 只对 formula 提供逐包统计接口">—</span>
                </template>
                <template v-else-if="statsLoading">统计中…</template>
                <template v-else>{{ formatNumber(stats?.installs30d) }}</template>
                <span
                  v-if="detail?.kind === 'formula' && !statsLoading && stats?.error"
                  class="text-caption text-muted-foreground"
                  :title="stats.error"
                >（取不到）</span>
              </dd>

              <template v-if="detail?.license">
                <dt class="text-muted-foreground">许可证</dt>
                <dd class="wrap-token font-mono">{{ detail.license }}</dd>
              </template>

              <template v-if="detail?.homepage">
                <dt class="text-muted-foreground">主页</dt>
                <dd class="wrap-token font-mono text-accent">{{ detail.homepage }}</dd>
              </template>

              <template v-if="detail?.tap">
                <dt class="text-muted-foreground">来源 tap</dt>
                <dd class="wrap-token font-mono">{{ detail.tap }}</dd>
              </template>
            </dl>

            <p
              v-if="detail?.autoUpdates"
              class="wrap-token rounded-control border border-border bg-muted px-3 py-2 text-label text-muted-foreground"
            >
              这个 cask 自带更新器，通常应该用应用内的「检查更新」，而不是 brew upgrade。
            </p>

            <div v-if="detail && detail.dependencies.length > 0" class="border-t border-border pt-3">
              <p class="label-mini mb-2">依赖 {{ detail.dependencies.length }} 个</p>
              <ul class="flex flex-wrap gap-1.5">
                <li v-for="name in detail.dependencies" :key="name">
                  <AppTag :label="name" />
                </li>
              </ul>
            </div>

            <div v-if="detail && detail.conflicts.length > 0" class="border-t border-border pt-3">
              <p class="label-mini mb-2">冲突</p>
              <ul class="flex flex-wrap gap-1.5">
                <li v-for="name in detail.conflicts" :key="name">
                  <AppTag :label="name" />
                </li>
              </ul>
            </div>

            <div v-if="detail?.caveats" class="border-t border-border pt-3">
              <p class="label-mini mb-2">安装提示</p>
              <pre class="wrap-token rounded-control bg-muted p-2 font-mono text-caption whitespace-pre-wrap">{{ detail.caveats }}</pre>
            </div>

            <div v-if="detail?.appPath && detail.installed" class="border-t border-border pt-3">
              <p class="label-mini mb-2">应用</p>
              <div class="mb-2 flex flex-wrap gap-1.5">
                <AppButton size="sm" @click="openTarget('open')">
                  <PhArrowSquareOut :size="13" aria-hidden="true" />
                  打开
                </AppButton>
                <AppButton size="sm" @click="openTarget('reveal')">
                  <PhFolderOpen :size="13" aria-hidden="true" />
                  在访达中显示
                </AppButton>
              </div>
              <p class="wrap-token font-mono text-caption text-muted-foreground">{{ detail.appPath }}</p>
            </div>

            <p v-if="notice" class="wrap-token rounded-control border border-border bg-muted px-3 py-2 text-label">
              {{ notice }}
            </p>

            <div class="border-t border-border pt-3">
              <div class="mb-2 flex flex-wrap items-center gap-1.5">
                <AppTag
                  v-if="installed"
                  label="升级到最新"
                  :active="action === 'upgrade'"
                  :title="upgradeAvailable ? undefined : 'brew 没报出新版本，先跑一次 brew update 再看'"
                  @click="action = 'upgrade'"
                />
                <AppTag
                  v-if="installed"
                  label="卸载"
                  :active="action === 'uninstall'"
                  @click="action = 'uninstall'"
                />
                <AppTag v-else label="安装" :active="action === 'install'" @click="action = 'install'" />
              </div>
              <ActionPanel
                :key="`brew:${selected.name}:${detail?.kind}:${action}`"
                source="brew"
                :action="action"
                :name="selected.name"
                :kind="detail?.kind ?? selected.kind"
                :upgrade-available="upgradeAvailable"
              />
            </div>
          </div>
        </AppDetailPane>
      </div>
    </div>

    <p class="text-caption text-muted-foreground">
      搜索与详情都走本机 brew 命令（带 <code class="font-mono">HOMEBREW_NO_AUTO_UPDATE=1</code>）。
      需要最新版本数据时，到诊断页跑一次「brew 更新索引」。
    </p>
  </div>
</template>
