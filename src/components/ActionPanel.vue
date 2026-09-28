<script setup lang="ts">
// 安装 / 卸载 / 升级的执行面板。
//
// 流程刻意做成"先看命令，再执行"：先展示将执行的完整命令，破坏性操作还要二次确认。
// 真正执行时交给后端的并发锁保证同一时刻只有一个任务，前端只是把按钮禁用掉。
//
// 日志与执行状态不在这个组件里（见 useActionSessions）：面板随选中项挂载销毁，
// 状态留在这儿就等于"切走即丢"。
import { computed, nextTick, onUnmounted, ref, watch } from "vue";
import { PhPlay, PhStopCircle } from "@phosphor-icons/vue";
import { invoke } from "@tauri-apps/api/core";

import AppButton from "./ui/AppButton.vue";
import LogViewer from "./ui/LogViewer.vue";
import StatusBadge from "./ui/StatusBadge.vue";
import {
  actionSession,
  cancelAction,
  startAction,
  type ActionTarget,
} from "../composables/useActionSessions";
import { isTauri } from "../lib/tauri";
import { useScrollLock } from "../composables/useScrollLock";
import { useSettings } from "../composables/useSettings";
import type { PackageAction, PlannedCommand } from "../types";

const props = defineProps<{
  source: string;
  action: PackageAction;
  name: string;
  version?: string | null;
  kind?: string | null;
  /** 升级操作专用：没有更新版本时置灰，避免出现"升级到最新"却无事可做 */
  upgradeAvailable?: boolean;
}>();

// 安装命令要跟设置页的数据源 / 代理一致：确认框里展示的就是最终会执行的那条
const { requestOptions } = useSettings();

const target = computed<ActionTarget>(() => ({
  source: props.source,
  action: props.action,
  name: props.name,
  version: props.version ?? null,
  kind: props.kind ?? null,
}));

/** 会话按目标共享：切到别的包再切回来，看到的还是同一条日志 */
const session = computed(() => actionSession(target.value));

const plan = ref<PlannedCommand | null>(null);
const planError = ref<string | null>(null);
const confirming = ref(false);

const upgradeBlocked = computed(() => props.action === "upgrade" && props.upgradeAvailable === false);
const cancelButton = ref<HTMLButtonElement | null>(null);

async function loadPlan() {
  plan.value = null;
  planError.value = null;
  if (!isTauri || !props.name) return;
  try {
    plan.value = await invoke<PlannedCommand>("plan_package_action", {
      source: props.source,
      action: props.action,
      name: props.name,
      version: props.version ?? null,
      kind: props.kind ?? null,
      admin: false,
      ...requestOptions(),
    });
  } catch (error) {
    planError.value = error instanceof Error ? error.message : String(error);
  }
}

// 这里只重取"将执行的命令"：日志与执行状态归会话管，不会因为面板重建被清空
watch(() => [props.source, props.action, props.name, props.version, props.kind], loadPlan, {
  immediate: true,
});

function start(admin: boolean) {
  confirming.value = false;
  void startAction(session.value, {
    admin,
    display: plan.value?.display ?? null,
    ...requestOptions(),
  });
}

async function openConfirm() {
  confirming.value = true;
  await nextTick();
  // 破坏性操作默认聚焦"取消"，避免误触回车直接执行
  cancelButton.value?.focus();
}

function cancel() {
  void cancelAction(session.value);
}

function onWindowKey(event: KeyboardEvent) {
  if (confirming.value && event.key === "Escape") confirming.value = false;
}

watch(confirming, (open) => {
  if (open) window.addEventListener("keydown", onWindowKey);
  else window.removeEventListener("keydown", onWindowKey);
});

onUnmounted(() => window.removeEventListener("keydown", onWindowKey));
useScrollLock(confirming);
</script>

<template>
  <div class="flex flex-col gap-2">
    <div class="flex flex-wrap items-center gap-2">
      <StatusBadge v-if="session.running" tone="busy" label="执行中" />
      <StatusBadge v-else-if="session.finished && session.exitCode === 0" tone="ok" label="完成" />
      <StatusBadge v-else-if="session.finished" tone="danger" label="失败" />

      <div class="ml-auto flex flex-wrap items-center gap-2">
        <AppButton v-if="session.running" variant="danger" @click="cancel">
          <PhStopCircle :size="14" aria-hidden="true" />
          取消
        </AppButton>

        <template v-else>
          <AppButton
            v-if="plan && !plan.destructive"
            variant="primary"
            :disabled="!isTauri"
            @click="start(false)"
          >
            <PhPlay :size="14" aria-hidden="true" />
            执行安装
          </AppButton>
          <AppButton
            v-else-if="plan"
            variant="danger"
            :disabled="upgradeBlocked"
            :title="upgradeBlocked ? 'registry 上没有更新的版本' : undefined"
            @click="openConfirm"
          >
            {{ action === "uninstall" ? "执行卸载" : "执行升级" }}
          </AppButton>
        </template>
      </div>
    </div>

    <p v-if="upgradeBlocked" class="text-caption text-muted-foreground">
      没有更新的版本，无需升级。
    </p>

    <p v-if="planError" class="rounded-control border border-danger px-3 py-2 text-danger" role="alert">
      {{ planError }}
    </p>

    <div v-else-if="plan" class="rounded-control border border-border bg-muted px-3 py-2">
      <p class="label-mini mb-1">将执行的命令</p>
      <p class="wrap-token font-mono text-label">{{ plan.display }}</p>
      <p v-if="plan.source === 'deno'" class="mt-1 text-caption text-muted-foreground">
        deno 的全局安装需要 <code class="font-mono">-A</code> 授权，等于把全部权限交给该包，请确认来源可信。
      </p>
    </div>

    <LogViewer
      v-if="session.lines.length > 0 || session.running"
      :lines="session.lines"
      height="12rem"
      placeholder="等待输出…"
    />

    <p
      v-if="session.permissionIssue && !session.running"
      class="flex flex-wrap items-center gap-2 rounded-control border border-border px-3 py-2"
    >
      <span class="min-w-0 flex-1 wrap-token">输出看起来是权限问题。可以改用管理员权限重试，系统会弹出授权框。</span>
      <AppButton @click="start(true)">以管理员权限重试</AppButton>
    </p>

    <p v-if="session.finished && session.exitCode === 0" class="text-caption text-muted-foreground">
      已{{ action === "install" ? "安装" : action === "uninstall" ? "卸载" : "升级" }}完成。
      切到「已安装」页会重新扫描，能看到最新结果。
    </p>

    <!-- 破坏性操作的二次确认：用模态而不是行内按钮，避免与旁边的控件混淆 -->
    <div
      v-if="confirming"
      class="fixed inset-0 z-1000 flex items-center justify-center bg-foreground/20 p-6 backdrop-blur-sm"
      @click.self="confirming = false"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        class="w-full max-w-sm rounded-card border border-border bg-surface p-4"
      >
        <h3 id="confirm-title" class="mb-2 text-title font-medium">
          确认{{ action === "uninstall" ? "卸载" : "升级" }} {{ name }}？
        </h3>
        <p class="mb-2 text-muted-foreground">
          {{ action === "uninstall" ? "卸载后该命令会从系统中移除。" : "将按 registry 上的最新版本重新安装。" }}
        </p>
        <p class="mb-3 wrap-token rounded-control border border-border bg-muted px-3 py-2 font-mono text-label">
          {{ plan?.display }}
        </p>
        <div class="flex justify-end gap-2">
          <AppButton ref="cancelButton" @click="confirming = false">取消</AppButton>
          <AppButton variant="danger" @click="start(false)">
            确认{{ action === "uninstall" ? "卸载" : "升级" }}
          </AppButton>
        </div>
      </div>
    </div>
  </div>
</template>
