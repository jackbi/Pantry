<script setup lang="ts">
// 主导航。图标 + 文字标签同时存在（技能 nav-label-icon 规则），
// 分组标题 + 左侧强调条 + aria-current，结构参考 WailBrew 的侧栏。
import { PhBeerStein, PhGear, PhPackage, PhTerminal } from "@phosphor-icons/vue";
import type { Component } from "vue";

import type { NavGroup, NavKey } from "../types";

defineProps<{ groups: NavGroup[]; current: NavKey }>();
const emit = defineEmits<{ select: [key: NavKey] }>();

const ICONS: Record<NavKey, Component> = {
  node: PhPackage,
  brew: PhBeerStein,
  diagnostics: PhTerminal,
  settings: PhGear,
};
</script>

<template>
  <nav class="flex flex-col gap-4 p-2" aria-label="主导航">
    <div v-for="group in groups" :key="group.label" class="flex flex-col gap-0.5">
      <p class="px-3 pt-1 pb-1.5 label-mini">{{ group.label }}</p>
      <button
        v-for="item in group.items"
        :key="item.key"
        type="button"
        class="relative flex h-8 cursor-pointer items-center gap-2.5 rounded-control px-3 text-left transition-colors duration-150"
        :class="
          current === item.key
            ? 'bg-muted font-medium text-foreground active:bg-border'
            : 'text-muted-foreground hover:bg-muted hover:text-foreground active:bg-muted'
        "
        :aria-current="current === item.key ? 'page' : undefined"
        @click="emit('select', item.key)"
      >
        <span
          v-if="current === item.key"
          class="absolute top-1.5 bottom-1.5 left-0 w-0.5 bg-accent"
          aria-hidden="true"
        />
        <component :is="ICONS[item.key]" :size="15" aria-hidden="true" />
        <span class="text-body">{{ item.label }}</span>
      </button>
    </div>
  </nav>
</template>
