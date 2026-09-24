<script setup lang="ts">
// 主题切换：图标 + 文字，aria-pressed 暴露选中态。
import { PhDesktop, PhMoon, PhSun } from "@phosphor-icons/vue";
import type { Component } from "vue";

import { useTheme, type ThemeMode } from "../composables/useTheme";

const { mode, setMode } = useTheme();

const OPTIONS: { value: ThemeMode; label: string; icon: Component }[] = [
  { value: "light", label: "亮", icon: PhSun },
  { value: "dark", label: "暗", icon: PhMoon },
  { value: "system", label: "跟随", icon: PhDesktop },
];
</script>

<template>
  <div class="flex gap-1" role="group" aria-label="主题">
    <button
      v-for="option in OPTIONS"
      :key="option.value"
      type="button"
      class="flex size-7 cursor-pointer items-center justify-center rounded-control border transition-colors duration-150"
      :class="
        mode === option.value
          ? 'border-control-border bg-muted font-medium text-foreground'
          : 'border-transparent text-muted-foreground hover:bg-muted hover:text-foreground'
      "
      :aria-label="`主题：${option.label}`"
      :title="option.label"
      :aria-pressed="mode === option.value"
      @click="setMode(option.value)"
    >
      <component :is="option.icon" :size="15" aria-hidden="true" />
    </button>
  </div>
</template>
