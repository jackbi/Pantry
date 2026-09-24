<script setup lang="ts">
// 开关：整行可点，语义按 role="switch" 走，键盘用原生的 Space / Enter。
// 状态不靠颜色单独表意——滑块位置之外还有「开 / 关」文字。
import { computed, useId } from "vue";

const props = withDefaults(
  defineProps<{
    label: string;
    modelValue: boolean;
    hint?: string;
    /** 打开后属于有风险的行为时用 danger 着色 */
    tone?: "default" | "danger";
    disabled?: boolean;
  }>(),
  { tone: "default", disabled: false },
);

const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();

const id = useId();
const labelId = `${id}-label`;
const hintId = `${id}-hint`;

const track = computed(() =>
  props.modelValue
    ? props.tone === "danger"
      ? "bg-danger"
      : "bg-primary"
    : "bg-muted",
);

const knob = computed(() =>
  props.modelValue
    ? props.tone === "danger"
      ? "left-[18px] bg-surface"
      : "left-[18px] bg-primary-foreground"
    : "left-0.5 bg-muted-foreground",
);
</script>

<template>
  <!--
    整行就是一个按钮：标题、说明、滑块都是它的内容，点哪都能切换。
    标题与说明用 aria-labelledby / aria-describedby 指定，避免开关状态文字
    （开 / 关）混进可访问名称。
  -->
  <button
    :id="id"
    type="button"
    role="switch"
    :aria-checked="modelValue"
    :aria-labelledby="labelId"
    :aria-describedby="hint ? hintId : undefined"
    :disabled="disabled"
    class="flex w-full cursor-pointer items-start justify-between gap-4 rounded-control text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:cursor-not-allowed disabled:opacity-45"
    @click="emit('update:modelValue', !modelValue)"
  >
    <span class="min-w-0">
      <span :id="labelId" class="block text-label font-medium">{{ label }}</span>
      <span v-if="hint" :id="hintId" class="block text-caption text-muted-foreground">
        {{ hint }}
      </span>
    </span>
    <span class="flex shrink-0 items-center gap-2 pt-0.5" aria-hidden="true">
      <span class="label-mini">{{ modelValue ? "开" : "关" }}</span>
      <span
        class="relative inline-block h-5 w-9 rounded-full border border-control-border transition-colors duration-150"
        :class="track"
      >
        <span class="absolute top-0.5 size-3.5 rounded-full transition-all duration-150" :class="knob" />
      </span>
    </span>
  </button>
</template>
