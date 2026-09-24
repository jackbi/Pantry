<script setup lang="ts">
// 设置页：包数据源与代理可编辑，保存后对所有查询与安装命令生效。
//
// 值存在前端（localStorage），每次调用命令时显式传给后端——后端不持有配置状态，
// 就不会出现「改了设置但某个模块还按旧值查询」这种分裂。
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

import AppButton from "../components/ui/AppButton.vue";
import AppInput from "../components/ui/AppInput.vue";
import AppPanel from "../components/ui/AppPanel.vue";
import AppSwitch from "../components/ui/AppSwitch.vue";
import StatusBadge from "../components/ui/StatusBadge.vue";
import ThemeSwitcher from "../components/ThemeSwitcher.vue";
import { useSettings } from "../composables/useSettings";
import { isTauri } from "../lib/tauri";
import type { MarketDefaults, MarketSettings, ProbeResult, ProxyReport } from "../types";

const { settings, save } = useSettings();

const defaults = ref<MarketDefaults | null>(null);
const proxy = ref<ProxyReport | null>(null);
const draft = ref<MarketSettings>({ ...settings.value });
const result = ref<ProbeResult | null>(null);
const probing = ref(false);
const error = ref<string | null>(null);
const saved = ref(false);

const dirty = computed(
  () =>
    draft.value.registry.trim() !== settings.value.registry ||
    draft.value.proxy.trim() !== settings.value.proxy ||
    draft.value.insecure !== settings.value.insecure,
);

const draftIsDefault = computed(
  () =>
    draft.value.registry.trim() === "" &&
    draft.value.proxy.trim() === "" &&
    !draft.value.insecure,
);

/** 探测用草稿值：可以先测再存，不必「先保存再验证」来回折腾 */
const effectiveRegistry = computed(
  () => draft.value.registry.trim() || defaults.value?.registry || "",
);

const autoProxy = computed(() => {
  if (!proxy.value) return "未检测到";
  return proxy.value.proxy ?? "未检测到";
});

const proxySourceLabel = computed(() => {
  switch (proxy.value?.source) {
    case "explicit":
      return "设置页填写";
    case "env":
      return "终端环境变量";
    case "system":
      return "macOS 系统设置";
    default:
      return "未检测到";
  }
});

function persist() {
  save(draft.value);
  draft.value = { ...settings.value };
  saved.value = true;
  result.value = null;
}

function discard() {
  draft.value = { ...settings.value };
  result.value = null;
  saved.value = false;
}

/**
 * 恢复默认＝把草稿恢复成「留空」：源用内置镜像，代理回到自动探测。
 * 只改草稿、不直接写入——三个按钮都走同一个保存模型，避免误点一下就
 * 悄悄覆盖掉已保存的配置。
 */
function restoreDefaults() {
  draft.value = { registry: "", proxy: "", insecure: false };
  saved.value = false;
  result.value = null;
}

async function probe() {
  if (!effectiveRegistry.value || probing.value) return;
  probing.value = true;
  error.value = null;
  try {
    result.value = await invoke<ProbeResult>("probe_registry", {
      url: `${effectiveRegistry.value.replace(/\/+$/, "")}/vue/latest`,
      proxy: draft.value.proxy.trim() || null,
      insecure: draft.value.insecure,
      timeoutSecs: 20,
    });
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    probing.value = false;
  }
}

onMounted(async () => {
  if (!isTauri) return;
  try {
    defaults.value = await invoke<MarketDefaults>("market_defaults");
    proxy.value = await invoke<ProxyReport>("proxy_report", { explicit: null });
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause);
  }
});
</script>

<template>
  <div class="mx-auto flex max-w-3xl flex-col gap-4 p-4">
    <header class="pt-6">
      <h1 class="text-heading font-semibold">设置</h1>
      <p class="text-muted-foreground">数据源、代理与界面偏好。</p>
    </header>

    <AppPanel title="外观" description="主题选择会立即生效并记住。">
      <ThemeSwitcher />
    </AppPanel>

    <AppPanel
      title="包数据源"
      description="只影响本应用自己的查询与安装，不会改写你的 .npmrc 或 brew 配置。"
    >
      <div class="flex flex-col gap-3">
        <AppInput
          v-model="draft.registry"
          label="Registry 地址"
          mono
          :placeholder="defaults?.registry ?? '留空使用内置默认源'"
          hint="留空即用内置默认源。安装 / 卸载 / 升级时通过 NPM_CONFIG_REGISTRY 传给 npm、pnpm、bun、deno。"
          described-by="registry-scope"
        />

        <div class="flex flex-wrap items-center gap-2">
          <AppButton
            v-if="defaults"
            size="sm"
            :title="defaults.registry"
            @click="draft.registry = defaults.registry"
          >
            内置默认源
          </AppButton>
          <AppButton size="sm" @click="draft.registry = 'https://registry.npmjs.org'">
            npm 官方源
          </AppButton>
          <AppButton :disabled="probing || !isTauri" size="sm" @click="probe">
            测试连接
          </AppButton>
          <StatusBadge v-if="probing" tone="busy" label="测试中" />
        </div>

        <p v-if="result" class="wrap-token text-caption" role="status">
          <StatusBadge
            :tone="result.ok ? 'ok' : 'danger'"
            :label="result.ok ? '可用' : '不可用'"
          />
          <span class="ml-2 text-muted-foreground">
            {{ result.status ? `HTTP ${result.status}` : "无响应" }} ·
            {{ result.elapsedMs }}ms ·
            {{ result.proxy ? `经代理 ${result.proxy}` : "直连" }}
            <template v-if="result.error"> · {{ result.error }}</template>
          </span>
        </p>
        <p v-if="error" class="text-caption text-danger" role="alert">{{ error }}</p>

        <p id="registry-scope" class="text-caption text-muted-foreground">
          下载量统计与 README 走独立域名
          <span class="font-mono">{{ defaults?.downloadsApi ?? "api.npmjs.org" }}</span>
          /
          <span class="font-mono">{{ defaults?.readmeCdn ?? "cdn.jsdelivr.net" }}</span>
          ，换镜像不会改变它们。npm 官方源在本机直连不通，需要同时启用下面的代理与跳过证书校验。
        </p>
      </div>
    </AppPanel>

    <AppPanel
      title="代理"
      description="留空时按「终端环境变量 → macOS 系统设置 → 直连」自动解析；填了就对所有出网请求生效（registry、下载量统计、README 与安装命令）。"
    >
      <div class="flex flex-col gap-3">
        <AppInput
          v-model="draft.proxy"
          label="代理地址"
          mono
          placeholder="http://127.0.0.1:7890"
          hint="从访达启动的 GUI 拿不到终端里的代理变量，需要在这里显式填写。"
        />

        <AppSwitch
          v-model="draft.insecure"
          tone="danger"
          label="跳过 TLS 证书校验"
          hint="仅在代理会拦截证书（如公司 CA）时使用；开启后本机所有请求都不再验证证书。"
        />

        <dl class="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 border-t border-border pt-3">
          <dt class="text-muted-foreground">自动检测结果</dt>
          <dd class="wrap-token font-mono text-body">{{ autoProxy }}</dd>
          <dt class="text-muted-foreground">来源</dt>
          <dd>
            <StatusBadge
              :tone="proxy?.proxy ? 'ok' : 'neutral'"
              :label="proxySourceLabel"
            />
          </dd>
          <dt class="text-muted-foreground">环境变量</dt>
          <dd class="wrap-token font-mono text-body">{{ proxy?.fromEnv ?? "无" }}</dd>
          <dt class="text-muted-foreground">系统设置</dt>
          <dd class="wrap-token font-mono text-body">{{ proxy?.fromSystem ?? "无" }}</dd>
        </dl>
      </div>
    </AppPanel>

    <div class="flex items-center gap-2">
      <AppButton variant="primary" :disabled="!dirty" @click="persist">保存设置</AppButton>
      <AppButton :disabled="!dirty" @click="discard">放弃改动</AppButton>
      <AppButton variant="ghost" :disabled="draftIsDefault" @click="restoreDefaults">
        恢复默认
      </AppButton>
      <span v-if="dirty" class="text-caption text-muted-foreground">有未保存的改动</span>
      <span v-else-if="saved" class="text-caption text-muted-foreground" role="status">
        已保存，立即生效
      </span>
    </div>
  </div>
</template>
