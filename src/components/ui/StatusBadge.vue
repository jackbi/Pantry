<script setup lang="ts">
// 状态必须"图标 + 文字"，不允许只靠颜色表意（技能 color-not-only 规则）。
import { computed } from "vue";
import {
  PhCheckCircle,
  PhCircleNotch,
  PhMinusCircle,
  PhWarningCircle,
  PhXCircle,
} from "@phosphor-icons/vue";

import { tokenFont } from "../../lib/text";

type Tone = "ok" | "warn" | "danger" | "neutral" | "busy";

const props = defineProps<{ tone: Tone; label: string }>();

const ICONS = {
  ok: PhCheckCircle,
  warn: PhWarningCircle,
  danger: PhXCircle,
  neutral: PhMinusCircle,
  busy: PhCircleNotch,
} as const;

const TONES: Record<Tone, string> = {
  ok: "text-accent",
  warn: "text-foreground",
  danger: "text-danger",
  neutral: "text-muted-foreground",
  busy: "text-primary",
};

const icon = computed(() => ICONS[props.tone]);
</script>

<template>
  <span class="inline-flex items-center gap-1" :class="TONES[tone]">
    <component
      :is="icon"
      :size="14"
      :class="tone === 'busy' ? 'animate-spin' : ''"
      aria-hidden="true"
    />
    <!-- 中文标签不套等宽：JetBrains Mono 没有中文字形（见 MASTER.md 的排版规则） -->
    <span class="text-label" :class="tokenFont(label)">{{ label }}</span>
  </span>
</template>
