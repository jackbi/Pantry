<script setup lang="ts">
// 命令面板（⌘K / Ctrl+K）：键盘优先的视图跳转，参考 npmx 的 `jump to…`。
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { PhBeerStein, PhGear, PhPackage, PhTerminal } from "@phosphor-icons/vue";
import type { Component } from "vue";

import AppKeycap from "./ui/AppKeycap.vue";
import { useScrollLock } from "../composables/useScrollLock";
import type { NavKey } from "../types";

const props = defineProps<{ open: boolean; items: { key: NavKey; label: string }[] }>();
const emit = defineEmits<{ close: []; select: [key: NavKey] }>();

const ICONS: Record<NavKey, Component> = {
  node: PhPackage,
  brew: PhBeerStein,
  diagnostics: PhTerminal,
  settings: PhGear,
};

const query = ref("");
const activeIndex = ref(0);
const input = ref<HTMLInputElement | null>(null);
useScrollLock(computed(() => props.open));

const results = computed(() => {
  const keyword = query.value.trim().toLowerCase();
  if (!keyword) return props.items;
  return props.items.filter((item) => item.label.toLowerCase().includes(keyword));
});

const listId = "palette-list";
/** 焦点留在输入框，用 aria-activedescendant 告诉读屏软件当前高亮哪一项 */
const activeDescendant = computed(() => {
  const item = results.value[activeIndex.value];
  return item ? optionId(item.key) : undefined;
});

function optionId(key: NavKey): string {
  return `palette-option-${key}`;
}

watch(
  () => props.open,
  async (open) => {
    if (!open) return;
    query.value = "";
    activeIndex.value = 0;
    await nextTick();
    input.value?.focus();
  },
);

watch(results, () => {
  activeIndex.value = 0;
});

function move(step: number) {
  if (results.value.length === 0) return;
  activeIndex.value = (activeIndex.value + step + results.value.length) % results.value.length;
}

function confirm() {
  const item = results.value[activeIndex.value];
  if (!item) return;
  emit("select", item.key);
  emit("close");
}

// 焦点不在输入框时也要能按 Esc 关闭
function onWindowKey(event: KeyboardEvent) {
  if (props.open && event.key === "Escape") emit("close");
}

onMounted(() => window.addEventListener("keydown", onWindowKey));
onUnmounted(() => window.removeEventListener("keydown", onWindowKey));
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-1000 flex items-start justify-center bg-foreground/20 p-24 backdrop-blur-sm"
    @click.self="emit('close')"
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-label="跳转到"
      class="w-full max-w-md overflow-hidden rounded-card border border-border bg-surface"
    >
      <!-- 输入框自己不带焦点环，改由整行描环（与 AppSearchField 同一套做法） -->
      <div
        class="flex items-center gap-2 border-b border-border px-3 focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-ring"
      >
        <span aria-hidden="true" class="font-mono text-muted-foreground">/</span>
        <input
          ref="input"
          v-model="query"
          type="text"
          role="combobox"
          :aria-expanded="results.length > 0"
          :aria-controls="results.length > 0 ? listId : undefined"
          :aria-activedescendant="activeDescendant"
          aria-autocomplete="list"
          class="h-10 min-w-0 flex-1 bg-transparent font-mono text-body outline-none placeholder:text-muted-foreground/70"
          placeholder="跳转到…"
          @keydown.down.prevent="move(1)"
          @keydown.up.prevent="move(-1)"
          @keydown.enter.prevent="confirm"
          @keydown.esc.prevent="emit('close')"
        />
        <AppKeycap keys="esc" />
      </div>

      <ul v-if="results.length > 0" :id="listId" role="listbox" aria-label="跳转结果" class="max-h-72 overflow-auto py-1">
        <li v-for="(item, index) in results" :key="item.key" role="presentation">
          <button
            :id="optionId(item.key)"
            role="option"
            :aria-selected="index === activeIndex"
            type="button"
            class="flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left transition-colors duration-150"
            :class="index === activeIndex ? 'bg-muted' : 'hover:bg-muted'"
            @mouseenter="activeIndex = index"
            @click="confirm()"
          >
            <component :is="ICONS[item.key]" :size="15" aria-hidden="true" />
            <span>{{ item.label }}</span>
          </button>
        </li>
      </ul>
      <p v-else class="px-3 py-6 text-center text-muted-foreground">没有匹配的页面</p>
    </div>
  </div>
</template>
