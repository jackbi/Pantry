<script setup lang="ts">
// 详情面板：参考 WailBrew 表格下方那块区域，未选中时给明确空态而不是留白。
import type { Component } from "vue";

import AppEmptyState from "./AppEmptyState.vue";

defineProps<{
  title: string;
  empty: boolean;
  emptyTitle: string;
  emptyDescription?: string;
  icon?: Component;
}>();
</script>

<template>
  <section class="rounded-card border border-border bg-surface">
    <header class="flex items-center justify-between gap-3 border-b border-border px-3 py-2">
      <h2 class="label-mini">{{ title }}</h2>
      <slot name="actions" />
    </header>
    <AppEmptyState
      v-if="empty"
      :icon="icon"
      :title="emptyTitle"
      :description="emptyDescription"
      compact
    />
    <div v-else class="p-3">
      <slot />
    </div>
  </section>
</template>
