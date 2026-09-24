<script setup lang="ts">
// Homebrew 页：已装的 formula / cask 与 brew 搜索放在一起。
// 入口只在 macOS 且能找到 brew 时出现（判断见 App.vue）。
import { ref } from "vue";
import { PhBeerStein } from "@phosphor-icons/vue";

import AppTabs from "../components/ui/AppTabs.vue";
import BrewView from "./BrewView.vue";
import InstalledView from "./InstalledView.vue";

defineProps<{ version: string | null }>();

type Mode = "installed" | "market";

const mode = ref<Mode>("installed");
const TABS = [
  { key: "installed", label: "已安装" },
  { key: "market", label: "安装新包" },
];
</script>

<template>
  <div class="mx-auto flex max-w-6xl flex-col gap-4 p-4">
    <header class="flex flex-col gap-1 pt-6">
      <h1 class="flex items-center gap-2 text-heading font-semibold">
        <PhBeerStein :size="20" aria-hidden="true" />
        Homebrew
      </h1>
      <p class="text-muted-foreground">
        已装的 formula 与 cask，以及按名字搜索安装新包。
        <span v-if="version" class="font-mono">{{ version }}</span>
      </p>
    </header>

    <AppTabs :tabs="TABS" :current="mode" label="Homebrew 视图" @select="mode = $event as Mode" />

    <InstalledView v-if="mode === 'installed'" scope="brew" />
    <BrewView v-else />
  </div>
</template>
