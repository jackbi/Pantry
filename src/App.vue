<script setup lang="ts">
// 应用外壳：顶栏（标识 / 命令面板 / 主题）+ 分组侧栏 + 视图容器。
//
// 导航按生态分组：Node 包（npm/pnpm/bun/deno）与 Homebrew，各自内部再分
// 「已安装 / 安装新包」。Homebrew 入口只在 macOS 且能找到 brew 时出现——
// 没装 brew 的机器上留一个点进去全是错误的入口没有意义。
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";

import AppSidebar from "./components/AppSidebar.vue";
import CommandPalette from "./components/CommandPalette.vue";
import ThemeSwitcher from "./components/ThemeSwitcher.vue";
import AppKeycap from "./components/ui/AppKeycap.vue";
import DiagnosticsView from "./views/DiagnosticsView.vue";
import HomebrewView from "./views/HomebrewView.vue";
import NodeView from "./views/NodeView.vue";
import SettingsView from "./views/SettingsView.vue";
import { isTauri } from "./lib/tauri";
import type { BrewAvailability, NavGroup, NavKey } from "./types";

const brew = ref<BrewAvailability | null>(null);

const groups = computed<NavGroup[]>(() => [
  {
    label: "浏览",
    items: [
      { key: "node", label: "Node 包" },
      // 只在 macOS 且 brew 可用时出现
      ...(brew.value?.platform === "macos" && brew.value.available
        ? [{ key: "brew" as NavKey, label: "Homebrew" }]
        : []),
    ],
  },
  {
    label: "系统",
    items: [
      { key: "diagnostics", label: "诊断" },
      { key: "settings", label: "设置" },
    ],
  },
]);

const current = ref<NavKey>("node");
const paletteOpen = ref(false);
const main = ref<HTMLElement | null>(null);

// 切换视图后把焦点交给主内容区，键盘与读屏用户不会被留在侧栏（技能 focus-on-route-change）
watch(current, async () => {
  await nextTick();
  main.value?.focus();
});

function onKeydown(event: KeyboardEvent) {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    paletteOpen.value = !paletteOpen.value;
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));

onMounted(async () => {
  if (!isTauri) return;
  try {
    brew.value = await invoke<BrewAvailability>("brew_availability");
  } catch {
    // 探测失败就当作没有 brew，入口不出现
    brew.value = null;
  }
});

// 入口消失时（例如换了机器）不能停留在已经不可见的页面上
watch(groups, (list) => {
  const keys = list.flatMap((group) => group.items.map((item) => item.key));
  if (!keys.includes(current.value)) current.value = "node";
});
</script>

<template>
  <div class="flex h-screen flex-col overflow-hidden bg-background text-foreground">
    <a
      href="#main"
      class="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-1000 focus:rounded-control focus:border focus:border-control-border focus:bg-surface focus:px-3 focus:py-1.5"
    >
      跳到主内容
    </a>

    <header class="flex h-11 shrink-0 items-center gap-3 border-b border-border px-3">
      <p class="font-mono text-title">
        <span class="text-accent">./</span><span class="font-medium">pantry</span>
      </p>

      <div class="ml-auto flex items-center gap-2">
        <button
          type="button"
          class="flex h-7 cursor-pointer items-center gap-2 rounded-control border border-border px-2 text-label text-muted-foreground transition-colors duration-150 hover:border-control-border hover:text-foreground active:bg-muted"
          @click="paletteOpen = true"
        >
          跳转…
          <AppKeycap keys="⌘K" />
        </button>
        <ThemeSwitcher />
      </div>
    </header>

    <div class="flex min-h-0 flex-1">
      <aside class="flex w-52 shrink-0 flex-col border-r border-border bg-surface">
        <AppSidebar :groups="groups" :current="current" @select="current = $event" class="pt-2" />

        <div class="mt-auto border-t border-border px-3 py-2">
          <p class="font-mono text-caption text-muted-foreground">v0.1.0 · alpha</p>
        </div>
      </aside>

      <!--
        relative 是必需的，不是装饰：Tailwind 的 sr-only 用 position:absolute，
        若没有已定位的祖先，它会相对初始包含块定位，把静态位置算进文档坐标，
        从而在外层撑出滚动条（表现为"最外层出现滚动条"）。
      -->
      <main
        id="main"
        ref="main"
        tabindex="-1"
        class="relative flex-1 overflow-y-auto focus:outline-none"
      >
        <NodeView v-if="current === 'node'" />
        <HomebrewView v-else-if="current === 'brew'" :version="brew?.version ?? null" />
        <SettingsView v-else-if="current === 'settings'" />
        <DiagnosticsView v-else />
      </main>
    </div>

    <CommandPalette
      :open="paletteOpen"
      :items="groups.flatMap((group) => group.items)"
      @close="paletteOpen = false"
      @select="current = $event"
    />
  </div>
</template>
