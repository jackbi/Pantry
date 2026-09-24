<script setup lang="ts">
// 面板容器：标题 + 说明 + 右侧操作区，避免每个页面各写一遍卡片样式。
defineProps<{ title: string; description?: string }>();
</script>

<template>
  <section class="rounded-card border border-border bg-surface">
    <header class="flex items-start justify-between gap-3 border-b border-border px-3 py-2.5">
      <div class="min-w-0">
        <h2 class="text-title font-medium">{{ title }}</h2>
        <p v-if="description" class="text-label text-muted-foreground">{{ description }}</p>
      </div>
      <div class="flex shrink-0 items-center gap-2">
        <slot name="actions" />
      </div>
    </header>
    <div class="p-3">
      <slot />
    </div>
  </section>
</template>
