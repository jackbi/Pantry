<script setup lang="ts">
// 统一按钮：变体 + 尺寸 + 四态（hover / focus-visible / active / disabled）。
// 组件里只允许出现语义类名，具体数值全部来自 theme.css 的 token。
import { computed, ref } from "vue";

const props = withDefaults(
  defineProps<{
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: "sm" | "md";
    type?: "button" | "submit";
    disabled?: boolean;
    loading?: boolean;
    /** 作为开关按钮时传 true，会输出 aria-pressed */
    pressed?: boolean;
    block?: boolean;
  }>(),
  {
    variant: "secondary",
    size: "md",
    type: "button",
    disabled: false,
    loading: false,
    block: false,
  },
);

const emit = defineEmits<{ click: [event: MouseEvent] }>();

const element = ref<HTMLButtonElement | null>(null);
// 供调用方在模态框里把初始焦点放到"取消"这类安全按钮上
defineExpose({ focus: () => element.value?.focus() });

const VARIANTS = {
  primary: "bg-primary text-primary-foreground hover:opacity-90 active:opacity-80",
  secondary: "border border-control-border bg-surface hover:bg-muted active:bg-muted",
  ghost:
    "border border-transparent text-muted-foreground hover:bg-muted hover:text-foreground active:bg-muted",
  danger: "border border-danger text-danger hover:bg-danger/10 active:bg-danger/20",
} as const;

const SIZES = {
  sm: "h-7 px-2.5 text-label",
  md: "h-8 px-3 text-body",
} as const;

const classes = computed(() => [
  // 不用 font-mono：按钮标签以中文为主，而 MASTER 规定等宽只用于拉丁与数字。
  // 需要等宽标签时由调用方传 font-mono（会合并到根元素上）。
  "inline-flex cursor-pointer items-center justify-center gap-1.5 rounded-control font-medium",
  "transition-colors duration-150 select-none",
  "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring",
  "disabled:cursor-not-allowed disabled:opacity-45",
  VARIANTS[props.variant],
  SIZES[props.size],
  props.block ? "w-full" : "",
]);

function onClick(event: MouseEvent) {
  if (props.disabled || props.loading) return;
  emit("click", event);
}
</script>

<template>
  <button
    ref="element"
    :type="type"
    :class="classes"
    :disabled="disabled || loading"
    :aria-busy="loading || undefined"
    :aria-pressed="pressed"
    @click="onClick"
  >
    <span
      v-if="loading"
      class="size-icon-sm animate-spin rounded-full border-2 border-current border-t-transparent"
      aria-hidden="true"
    />
    <slot />
  </button>
</template>
