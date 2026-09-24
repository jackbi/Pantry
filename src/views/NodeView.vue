<script setup lang="ts">
// Node 生态页：一个入口装下「已安装的 npm/pnpm/bun/deno 全局包」与「npm 市场」。
//
// 为什么用两个视图而不是一张混合列表：两边的数据来源与操作完全不同——
// 已安装侧有来源筛选、体积、磁盘占用，市场侧有安装方式选择与分页。
// 合成一张表会让筛选和排序都失去意义，所以并到同一页、用视图切换分开。
import { ref } from "vue";

import AppTabs from "../components/ui/AppTabs.vue";
import InstalledView from "./InstalledView.vue";
import MarketView from "./MarketView.vue";

type Mode = "installed" | "market";

const mode = ref<Mode>("installed");
const TABS = [
  { key: "installed", label: "已安装" },
  { key: "market", label: "安装新包" },
];
</script>

<template>
  <div class="mx-auto flex max-w-6xl flex-col gap-4 p-4">
    <header class="pt-6">
      <h1 class="text-heading font-semibold">Node 包</h1>
      <p class="text-muted-foreground">
        npm、pnpm、bun、deno 的全局包：先看装了什么，再到 npm 市场找新的。
      </p>
    </header>

    <AppTabs :tabs="TABS" :current="mode" label="Node 包视图" @select="mode = $event as Mode" />

    <InstalledView v-if="mode === 'installed'" scope="node" />
    <MarketView v-else />
  </div>
</template>
