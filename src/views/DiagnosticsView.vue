<script setup lang="ts">
// 诊断页：默认只回答三个问题——包管理器在不在、什么版本、有没有新版本。
//
// PATH 解析、代理与连通性、命令试跑是排障用的细节（阶段 0 的沉淀），
// 折进「环境详情」里按需展开，不再和主信息一起堆在首屏。
import { onMounted, onUnmounted, ref } from "vue";
import { computed, nextTick } from "vue";
import {
  PhArrowClockwise,
  PhCaretDown,
  PhCaretRight,
  PhStopCircle,
} from "@phosphor-icons/vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import AppButton from "../components/ui/AppButton.vue";
import AppInput from "../components/ui/AppInput.vue";
import AppPanel from "../components/ui/AppPanel.vue";
import AppTable from "../components/ui/AppTable.vue";
import LogViewer from "../components/ui/LogViewer.vue";
import StatusBadge from "../components/ui/StatusBadge.vue";
import { useScrollLock } from "../composables/useScrollLock";
import { useSettings } from "../composables/useSettings";
import { isTauri } from "../lib/tauri";
import { formatNumber } from "../lib/format";
import type {
  ManagerStatus,
  PathReport,
  ProbeResult,
  ProcEvent,
  ProxyReport,
  TableColumn,
  TableRow,
} from "../types";

type LogLine = { stream: string; line: string };
type TaskSummary = { code: number | null; killed: boolean; elapsedMs: number };

const MAX_LOG_LINES = 1000;

// 自检要跟设置页用同一个源：否则"市场能搜到、诊断说查不到最新版"会自相矛盾
const { settings, requestOptions } = useSettings();

const columns: TableColumn[] = [
  { key: "name", label: "包管理器", mono: true, width: "7rem" },
  { key: "state", label: "状态", width: "6rem" },
  { key: "version", label: "已安装版本", mono: true, width: "9rem" },
  { key: "latest", label: "最新版本", mono: true, width: "12rem" },
  { key: "path", label: "可执行文件", mono: true },
  { key: "action", label: "操作", width: "5.5rem" },
];

const managers = ref<ManagerStatus[]>([]);
const managersLoading = ref(false);
const managersError = ref<string | null>(null);

const updateTarget = ref<ManagerStatus | null>(null);
const pendingUpdate = ref<string | null>(null);
const updateResult = ref<string | null>(null);

useScrollLock(computed(() => updateTarget.value !== null));

const envOpen = ref(false);
const busy = ref(false);
const pathReport = ref<PathReport | null>(null);
const diagnosticsError = ref<string | null>(null);

const running = ref(false);
const currentId = ref<string | null>(null);
const startedInfo = ref<{ executable: string; pid: number } | null>(null);
const logLines = ref<LogLine[]>([]);
const lastExit = ref<TaskSummary | null>(null);

const proxyReport = ref<ProxyReport | null>(null);
const probes = ref<ProbeResult[]>([]);
const probing = ref(false);

const customProgram = ref("npm");
const customArgs = ref("--version");

const presets = [
  { label: "npm 全局包", program: "npm", args: ["ls", "-g", "--depth=0", "--json"] },
  { label: "pnpm 全局包", program: "pnpm", args: ["ls", "-g", "--depth=0", "--json"] },
  { label: "bun 全局包", program: "bun", args: ["pm", "ls", "-g"] },
  { label: "brew 已装", program: "brew", args: ["list", "--formula"] },
  // 应用的其它 brew 调用都带 HOMEBREW_NO_AUTO_UPDATE=1（自动更新可能几十秒），
  // 需要新索引时在这里显式跑一次
  { label: "brew 更新索引", program: "brew", args: ["update"] },
];

const rows = ref<TableRow[]>([]);

function toRows(list: ManagerStatus[]): TableRow[] {
  return list.map((item) => ({
    id: item.name,
    name: item.name,
    state: item.available ? "已安装" : "未安装",
    version: item.version ?? "—",
    latest: item.selfUpdating ? "—" : item.latest ?? "—",
    path: item.path ?? "—",
    action: "",
    status: item,
  }));
}

/** 有新版才给更新按钮；brew 是自更新项，始终可点 */
function canUpdate(item: ManagerStatus): boolean {
  return Boolean(item.update) && (item.upgradeAvailable || item.selfUpdating);
}

const updateDisplay = computed(() => updateTarget.value?.update?.display ?? "");
/** Homebrew 装的 npm 系用自己更新会写进 Homebrew 的目录，提示改用 brew（brew 自己不算） */
const updateLooksBrewManaged = computed(() => {
  const target = updateTarget.value;
  if (!target || target.name === "brew") return false;
  return /\/opt\/homebrew\/|\/usr\/local\/Cellar\//.test(target.path ?? "");
});

async function loadManagers() {
  if (!isTauri) return;
  managersLoading.value = true;
  managersError.value = null;
  try {
    managers.value = await invoke<ManagerStatus[]>("manager_status", requestOptions());
    rows.value = toRows(managers.value);
  } catch (error) {
    managersError.value = error instanceof Error ? error.message : String(error);
  } finally {
    managersLoading.value = false;
  }
}

const upgradable = () => managers.value.filter((item) => item.upgradeAvailable);

async function diagnose() {
  busy.value = true;
  diagnosticsError.value = null;
  try {
    pathReport.value = await invoke<PathReport>("diagnose_environment");
    proxyReport.value = await invoke<ProxyReport>("proxy_report", { explicit: null });
  } catch (error) {
    diagnosticsError.value = error instanceof Error ? error.message : String(error);
  } finally {
    busy.value = false;
  }
}

/** 折叠区按需加载：默认视图不查 PATH 也不探测代理 */
async function toggleEnv() {
  envOpen.value = !envOpen.value;
  if (envOpen.value && !pathReport.value) await diagnose();
}

function pushLog(stream: string, line: string) {
  logLines.value.push({ stream, line });
  if (logLines.value.length > MAX_LOG_LINES) {
    logLines.value.splice(0, logLines.value.length - MAX_LOG_LINES);
  }
}

async function run(program: string, args: string[]) {
  if (running.value || !program) return;
  logLines.value = [];
  lastExit.value = null;
  startedInfo.value = null;
  try {
    // 带上设置页的代理：brew update / npm 这类命令在访达启动的 GUI 里拿不到终端变量
    const options = requestOptions();
    const id = await invoke<string>("run_command", {
      program,
      args,
      cwd: null,
      proxy: options.proxy,
      insecure: options.insecure,
    });
    currentId.value = id;
    running.value = true;
  } catch (error) {
    pushLog("stderr", error instanceof Error ? error.message : String(error));
  }
}

async function runCustom() {
  const args = customArgs.value.trim() ? customArgs.value.trim().split(/\s+/) : [];
  await run(customProgram.value.trim(), args);
}

/** 更新按钮：先摆出命令，再执行；输出复用命令试跑那套流式日志 */
async function openUpdate(item: ManagerStatus) {
  if (!item.update) return;
  updateTarget.value = item;
  updateResult.value = null;
  logLines.value = [];
  lastExit.value = null;
  await nextTick();
}

async function startUpdate() {
  const command = updateTarget.value?.update;
  const name = updateTarget.value?.name;
  if (!command || !name) return;
  pendingUpdate.value = name;
  updateResult.value = null;
  await run(command.program, command.args);
}

function closeUpdate() {
  if (running.value) return;
  updateTarget.value = null;
  updateResult.value = null;
}

async function cancel() {
  if (!currentId.value) return;
  const killed = await invoke<boolean>("cancel_command", { id: currentId.value });
  if (!killed) pushLog("meta", "任务已结束，无需取消");
}

async function probe(url: string, insecure: boolean, proxy: string | null) {
  probing.value = true;
  try {
    const result = await invoke<ProbeResult>("probe_registry", {
      url,
      proxy,
      insecure,
      timeoutSecs: 20,
    });
    probes.value = [result, ...probes.value.filter((item) => item.url !== url)];
  } catch (error) {
    pushLog("stderr", error instanceof Error ? error.message : String(error));
  } finally {
    probing.value = false;
  }
}

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  if (!isTauri) return;
  try {
    unlisten = await listen<ProcEvent>("proc://event", ({ payload }) => {
      if (payload.kind === "started") {
        startedInfo.value = { executable: payload.executable, pid: payload.pid };
        pushLog("meta", `启动 ${payload.program} → ${payload.executable}（pid ${payload.pid}）`);
      } else if (payload.kind === "output") {
        pushLog(payload.stream, payload.line);
      } else {
        lastExit.value = {
          code: payload.code,
          killed: payload.killed,
          elapsedMs: payload.elapsedMs,
        };
        running.value = false;
        currentId.value = null;
        pushLog(
          "meta",
          `退出：code=${payload.code ?? "被信号终止"} · killed=${payload.killed} · ${payload.elapsedMs}ms`,
        );
        // 更新成功后重新自检，把新版本读回来
        const updated = pendingUpdate.value;
        pendingUpdate.value = null;
        if (payload.code === 0 && updated) {
          void loadManagers().then(() => {
            const now = managers.value.find((item) => item.name === updated);
            updateResult.value = now?.version
              ? `${updated} 现在是 ${now.version}`
              : `${updated} 已更新`;
          });
        }
      }
    });
  } catch {
    // 事件通道拿不到只会让「命令试跑」收不到实时输出，自检本身不该被拖住
  }
  await loadManagers();
});

onUnmounted(() => {
  // unlisten 是 async：注销失败会变成 rejected promise，必须接住，
  // 否则离开页面时留下一条 unhandled rejection（事件通道不可用时就会发生）
  void Promise.resolve(unlisten?.()).catch(() => {});
});
</script>

<template>
  <div class="mx-auto flex max-w-5xl flex-col gap-4 p-4">
    <header class="flex items-end justify-between gap-4 pt-6">
      <div>
        <h1 class="text-heading font-semibold">诊断</h1>
        <p class="text-muted-foreground">
          检查 npm、pnpm、bun、deno、brew 在不在、什么版本、有没有新版本。
        </p>
      </div>
      <div class="flex shrink-0 items-center gap-3">
        <StatusBadge v-if="managersLoading" tone="busy" label="检测中" />
        <StatusBadge
          v-else-if="upgradable().length > 0"
          tone="warn"
          :label="`${upgradable().length} 个可升级`"
        />
        <AppButton :disabled="managersLoading || !isTauri" @click="loadManagers">
          <PhArrowClockwise :size="14" aria-hidden="true" />
          重新检测
        </AppButton>
      </div>
    </header>

    <p v-if="!isTauri" class="rounded-card border border-danger px-3 py-2 text-danger" role="alert">
      当前不在 Tauri 中运行，无法调用后端命令。请用
      <code class="font-mono">pnpm tauri:dev</code> 启动。
    </p>

    <p v-if="managersError" class="rounded-card border border-danger px-3 py-2 text-danger" role="alert">
      检测失败：{{ managersError }}
    </p>

    <div class="overflow-hidden rounded-card border border-border bg-surface">
      <AppTable caption="包管理器自检结果" :columns="columns" :rows="rows">
        <template #cell="{ row, column }">
          <template v-if="column.key === 'state'">
            <StatusBadge
              :tone="(row.status as ManagerStatus).available ? 'ok' : 'neutral'"
              :label="(row.status as ManagerStatus).available ? '已安装' : '未找到'"
            />
          </template>
          <template v-else-if="column.key === 'version'">
            <span class="font-mono text-label">{{ row.version }}</span>
          </template>
          <template v-else-if="column.key === 'latest'">
            <span
              v-if="(row.status as ManagerStatus).selfUpdating"
              class="text-caption text-muted-foreground"
              title="Homebrew 用 brew update 更新自己，不做版本对比"
            >brew update 自更新</span>
            <span
              v-else-if="row.name === 'node'"
              class="text-caption text-muted-foreground"
              title="node 的升级走 nvm / Homebrew 等方式，这里只读版本"
            >由 nvm / brew 等管理</span>
            <span v-else class="inline-flex items-center gap-2">
              <span class="font-mono text-label">{{ row.latest }}</span>
              <StatusBadge
                v-if="(row.status as ManagerStatus).upgradeAvailable"
                tone="warn"
                label="可升级"
              />
            </span>
          </template>
          <template v-else-if="column.key === 'path'">
            <span class="wrap-token text-caption text-muted-foreground">{{ row.path }}</span>
          </template>
          <template v-else-if="column.key === 'action'">
            <AppButton
              v-if="canUpdate(row.status as ManagerStatus)"
              size="sm"
              :disabled="running"
              :title="(row.status as ManagerStatus).update?.display"
              @click="openUpdate(row.status as ManagerStatus)"
            >
              更新
            </AppButton>
            <span v-else class="text-muted-foreground">—</span>
          </template>
          <template v-else>
            <span class="font-mono text-label">{{ row.name }}</span>
          </template>
        </template>
      </AppTable>
    </div>

    <p class="text-caption text-muted-foreground">
      点「更新」会先摆出将执行的命令再执行，输出实时显示、可取消：npm 是
      <code class="font-mono">npm install -g npm@latest</code>，pnpm 是
      <code class="font-mono">pnpm add -g pnpm@latest</code>，bun 与 deno 用各自的
      <code class="font-mono">upgrade</code>，brew 用 <code class="font-mono">brew update</code>。
      <template v-if="managers.some((item) => !item.available)">
        未找到的命令通常意味着没安装，或不在登录 shell 的 PATH 里（见下方环境详情）。
      </template>
    </p>

    <!-- 排障细节：默认收起，展开时才去查 PATH 与代理 -->
    <section class="rounded-card border border-border bg-surface">
      <button
        type="button"
        class="flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left transition-colors duration-150 hover:bg-muted active:bg-border"
        :aria-expanded="envOpen"
        @click="toggleEnv"
      >
        <component :is="envOpen ? PhCaretDown : PhCaretRight" :size="12" aria-hidden="true" />
        <span class="label-mini">环境详情</span>
        <span class="text-caption text-muted-foreground">
          PATH 解析、代理与连通性、命令试跑
        </span>
        <StatusBadge v-if="busy" tone="busy" label="检测中" class="ml-auto" />
      </button>

      <div v-if="envOpen" class="flex flex-col gap-4 border-t border-border p-3">
        <p v-if="diagnosticsError" class="text-danger" role="alert">{{ diagnosticsError }}</p>

        <AppPanel title="PATH 解析" description="PATH 来自登录 shell，缓存后传给所有子进程。">
          <template #actions>
            <AppButton :disabled="busy || !isTauri" size="sm" @click="diagnose">
              <PhArrowClockwise :size="13" aria-hidden="true" />
              重新检测
            </AppButton>
          </template>

          <template v-if="pathReport">
            <dl class="mb-3 grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1">
              <dt class="text-muted-foreground">解析来源</dt>
              <dd class="font-mono text-body">{{ pathReport.source }}</dd>
              <dt class="text-muted-foreground">登录 shell</dt>
              <dd class="wrap-token font-mono text-body">{{ pathReport.shell }}</dd>
              <dt class="text-muted-foreground">PATH 目录数</dt>
              <dd>{{ pathReport.resolved.length }}</dd>
            </dl>

            <ul class="flex flex-wrap gap-1.5">
              <li
                v-for="dir in pathReport.resolved"
                :key="dir"
                class="wrap-token rounded-full border border-border px-2.5 py-0.5 font-mono text-caption"
                :class="
                  pathReport.fallbacksAdded.includes(dir)
                    ? 'border-primary text-primary'
                    : 'text-muted-foreground'
                "
              >
                {{ dir }}<span v-if="pathReport.fallbacksAdded.includes(dir)"> · 兜底</span>
              </li>
            </ul>
          </template>
        </AppPanel>

        <AppPanel title="命令试跑" description="输出实时流式展示，可随时取消。">
          <template #actions>
            <StatusBadge v-if="running" tone="busy" label="运行中" />
            <AppButton variant="danger" :disabled="!running" @click="cancel">
              <PhStopCircle :size="14" aria-hidden="true" />
              取消
            </AppButton>
          </template>

          <div class="mb-3 flex flex-wrap gap-2">
            <AppButton
              v-for="preset in presets"
              :key="preset.label"
              size="sm"
              :disabled="running || !isTauri"
              @click="run(preset.program, preset.args)"
            >
              {{ preset.label }}
            </AppButton>
          </div>

          <!--
            说明文字放在整行下方，而不是塞进"参数"那一列：
            某一列多一行会让各列高度不等，items-end 就会把对应输入框顶上去（实测差 20px）。
          -->
          <form class="mb-3" @submit.prevent="runCustom">
            <div class="flex flex-wrap items-end gap-2">
              <div class="w-32">
                <AppInput v-model="customProgram" label="程序" mono placeholder="npm" />
              </div>
              <div class="min-w-45 flex-1">
                <AppInput
                  v-model="customArgs"
                  label="参数"
                  mono
                  placeholder="--version"
                  described-by="custom-args-hint"
                />
              </div>
              <AppButton
                type="submit"
                variant="primary"
                :disabled="running || !isTauri"
                :loading="running"
              >
                执行
              </AppButton>
            </div>
            <p id="custom-args-hint" class="mt-1.5 text-caption text-muted-foreground">
              参数按空格分隔，不做 shell 解析。
            </p>
          </form>

          <p v-if="startedInfo" class="mb-2 wrap-token font-mono text-caption text-muted-foreground">
            {{ startedInfo.executable }} · pid {{ startedInfo.pid }}
          </p>

          <LogViewer :lines="logLines" />

          <p v-if="lastExit" class="mt-2 text-muted-foreground">
            上次退出：code={{ lastExit.code ?? "被信号终止" }} · killed={{ lastExit.killed }} ·
            {{ lastExit.elapsedMs }}ms
          </p>
        </AppPanel>

        <AppPanel title="注册表连通性" description="结果会如实反映失败原因，不做静默降级。">
          <template #actions>
            <StatusBadge v-if="probing" tone="busy" label="探测中" />
          </template>

          <dl class="mb-3 grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1">
            <dt class="text-muted-foreground">代理</dt>
            <dd class="wrap-token font-mono text-body">{{ proxyReport?.proxy ?? "未检测到" }}</dd>
            <dt class="text-muted-foreground">来源</dt>
            <dd class="wrap-token">
              {{ proxyReport?.source ?? "-" }}（环境变量 {{ proxyReport?.fromEnv ?? "无" }} / 系统
              {{ proxyReport?.fromSystem ?? "无" }}）
            </dd>
          </dl>

          <div class="mb-3 flex flex-wrap gap-2">
            <AppButton
              size="sm"
              :disabled="probing || !isTauri"
              @click="probe('https://registry.npmmirror.com/vue/latest', false, null)"
            >
              直连 npmmirror
            </AppButton>
            <AppButton
              size="sm"
              :disabled="probing || !isTauri"
              @click="probe('https://registry.npmjs.org/vue/latest', false, null)"
            >
              npmjs 直连
            </AppButton>
            <AppButton
              size="sm"
              :disabled="probing || !isTauri"
              @click="
                probe(
                  'https://registry.npmjs.org/vue/latest',
                  true,
                  settings.proxy || proxyReport?.proxy || null,
                )
              "
            >
              npmjs 走代理 + 跳过证书
            </AppButton>
          </div>

          <p v-if="probes.length === 0" class="text-muted-foreground">
            还没有探测结果。上面三个按钮分别验证镜像直连、官方源直连、官方源走代理。
          </p>

          <table v-else class="w-full border-collapse">
            <caption class="sr-only">注册表探测结果</caption>
            <thead>
              <tr class="text-left text-muted-foreground">
                <th scope="col" class="border-b border-border py-2 pr-2 text-label font-medium">URL</th>
                <th scope="col" class="border-b border-border py-2 pr-2 text-label font-medium">结果</th>
                <th scope="col" class="border-b border-border py-2 text-label font-medium">耗时</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="item in probes" :key="item.url">
                <td class="wrap-token border-b border-border py-2 pr-2 font-mono text-caption">
                  {{ item.url }}
                </td>
                <td class="border-b border-border py-2 pr-2">
                  <StatusBadge
                    :tone="item.ok ? 'ok' : 'danger'"
                    :label="item.ok ? `HTTP ${item.status}` : `失败：${item.error ?? '未知原因'}`"
                  />
                </td>
                <td class="border-b border-border py-2 tabular-nums">{{ item.elapsedMs }}ms</td>
              </tr>
            </tbody>
          </table>
        </AppPanel>
      </div>
    </section>

    <p class="text-caption text-muted-foreground">
      自检共 {{ formatNumber(managers.length) }} 项，只读本机命令与 registry 的版本号，不安装任何东西。
    </p>

    <!-- 更新用弹层承载：命令、实时输出、取消都在一个地方，不与主列表抢空间 -->
    <div
      v-if="updateTarget"
      class="fixed inset-0 z-1000 flex items-center justify-center bg-foreground/20 p-6 backdrop-blur-sm"
      @click.self="closeUpdate"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-title"
        class="flex max-h-[80vh] w-full max-w-2xl flex-col overflow-hidden rounded-card border border-border bg-surface"
      >
        <header class="flex items-center justify-between gap-3 border-b border-border px-4 py-2.5">
          <h3 id="update-title" class="min-w-0 truncate font-mono text-title font-medium">
            更新 {{ updateTarget.name }}
          </h3>
          <div class="flex shrink-0 items-center gap-2">
            <StatusBadge v-if="running" tone="busy" label="执行中" />
            <AppButton v-if="running" variant="danger" @click="cancel">取消</AppButton>
            <AppButton v-else @click="closeUpdate">关闭</AppButton>
          </div>
        </header>

        <div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
          <dl class="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1">
            <dt class="text-muted-foreground">当前版本</dt>
            <dd class="font-mono">{{ updateTarget.version ?? "—" }}</dd>
            <dt class="text-muted-foreground">最新版本</dt>
            <dd class="font-mono">
              {{ updateTarget.selfUpdating ? "由 brew update 自行更新" : updateTarget.latest ?? "—" }}
            </dd>
            <dt class="text-muted-foreground">将执行</dt>
            <dd class="wrap-token font-mono text-accent">{{ updateDisplay }}</dd>
          </dl>

          <p
            v-if="updateLooksBrewManaged"
            class="wrap-token rounded-control border border-border bg-muted px-3 py-2 text-label text-muted-foreground"
          >
            这是 Homebrew 装的（{{ updateTarget.path }}）：自己更新会写进 Homebrew
            的目录，容易互相打架，用 <code class="font-mono">brew upgrade</code> 更稳妥。
          </p>

          <p
            v-if="updateResult"
            class="rounded-control border border-border bg-muted px-3 py-2 text-label"
          >
            {{ updateResult }}
          </p>

          <LogViewer :lines="logLines" class="min-h-40 flex-1" />

          <!-- 退出信息已经在日志里（LogViewer 的 meta 行），这里不再重复 -->
          <div class="flex justify-end gap-2">
            <AppButton
              variant="primary"
              :loading="running"
              :disabled="running || !isTauri"
              @click="startUpdate"
            >
              {{ lastExit ? "再执行一次" : "执行更新" }}
            </AppButton>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
