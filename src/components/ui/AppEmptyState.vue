<script setup lang="ts">
// 空态：图标 + 标题 + 说明 + 可选操作。技能要求空态必须给引导，不能留白。
import type { Component } from "vue";

defineProps<{ icon?: Component; title: string; description?: string; compact?: boolean }>();
</script>

<template>
  <div
    class="flex flex-col items-center justify-center gap-2 text-center"
    :class="compact ? 'px-4 py-6' : 'px-4 py-12'"
  >
    <component v-if="icon" :is="icon" :size="compact ? 20 : 28" class="text-muted-foreground" aria-hidden="true" />
    <p class="font-medium">{{ title }}</p>
    <p v-if="description" class="max-w-prose text-muted-foreground">{{ description }}</p>
    <div class="mt-1"><slot /></div>
  </div>
</template>
