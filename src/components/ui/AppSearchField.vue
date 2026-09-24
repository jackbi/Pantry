<script setup lang="ts">
// 搜索框：`/` 前缀 + 键帽提示 + 提交按钮，参考 npmx 的搜索主入口。
import { PhMagnifyingGlass } from "@phosphor-icons/vue";

import AppKeycap from "./AppKeycap.vue";

withDefaults(
  defineProps<{
    modelValue: string;
    label: string;
    placeholder?: string;
    shortcut?: string;
    size?: "md" | "lg";
    showButton?: boolean;
  }>(),
  { shortcut: "/", size: "md", showButton: true },
);

const emit = defineEmits<{ "update:modelValue": [value: string]; submit: [] }>();
</script>

<template>
  <form
    class="flex items-center gap-2 rounded-control border border-control-border bg-surface px-3 transition-colors duration-150 focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-ring"
    :class="size === 'lg' ? 'h-12' : 'h-9'"
    @submit.prevent="emit('submit')"
  >
    <label :for="`search-${label}`" class="sr-only">{{ label }}</label>
    <span aria-hidden="true" class="font-mono text-muted-foreground">/</span>
    <input
      :id="`search-${label}`"
      :value="modelValue"
      :placeholder="placeholder"
      type="search"
      class="min-w-0 flex-1 bg-transparent font-mono outline-none placeholder:text-muted-foreground/70"
      :class="size === 'lg' ? 'text-title' : 'text-body'"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    />
    <AppKeycap v-if="shortcut" :keys="shortcut" />
    <button
      v-if="showButton"
      type="submit"
      class="inline-flex h-7 cursor-pointer items-center gap-1 rounded-control bg-primary px-2.5 text-label text-primary-foreground transition-opacity duration-150 hover:opacity-90 active:opacity-80"
    >
      <PhMagnifyingGlass :size="13" aria-hidden="true" />
      搜索
    </button>
  </form>
</template>
