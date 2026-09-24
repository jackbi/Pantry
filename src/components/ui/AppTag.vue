<script setup lang="ts">
// 标签 / 筛选片。参考 npmx 的 `•vue •react` 生态标签。
// 传 active 时渲染为可按按钮（带 aria-pressed），不传则是纯展示标签。
import { tokenFont } from "../../lib/text";

withDefaults(
  defineProps<{
    label: string;
    dot?: boolean;
    active?: boolean;
    count?: number;
    disabled?: boolean;
    title?: string;
  }>(),
  { dot: false, disabled: false },
);
</script>

<template>
  <component
    :is="active === undefined ? 'span' : 'button'"
    :type="active === undefined ? undefined : 'button'"
    :disabled="active === undefined ? undefined : disabled"
    :title="title"
    class="inline-flex items-center gap-1.5 rounded-control border px-2 py-1 text-caption transition-colors duration-150"
    :class="
      [
        // 中文标签不套等宽（JetBrains Mono 没有中文字形）
        tokenFont(label),
        disabled
          ? 'cursor-not-allowed border-border text-muted-foreground opacity-45'
          : active
            ? 'cursor-pointer border-control-border bg-muted text-foreground'
            : 'cursor-pointer border-border text-muted-foreground hover:border-control-border hover:text-foreground',
      ]
    "
    :aria-pressed="active === undefined ? undefined : active"
  >
    <span v-if="dot" class="size-1 rounded-full bg-accent" aria-hidden="true" />
    {{ label }}
    <span v-if="count !== undefined" class="tabular-nums opacity-60">{{ count }}</span>
  </component>
</template>
