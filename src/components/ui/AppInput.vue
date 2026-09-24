<script setup lang="ts">
// 输入框强制带可见 label：技能把"只用 placeholder 当标签"列为反模式。
import { computed, useId } from "vue";

const props = withDefaults(
  defineProps<{
    label: string;
    modelValue: string;
    hint?: string;
    error?: string | null;
    /** 外部说明元素的 id，会并入 aria-describedby */
    describedBy?: string;
    placeholder?: string;
    mono?: boolean;
    disabled?: boolean;
  }>(),
  { mono: false, disabled: false },
);

const emit = defineEmits<{ "update:modelValue": [value: string] }>();

const id = useId();
const hintId = `${id}-hint`;
const errorId = `${id}-error`;

const describedBy = computed(() => {
  const ids: string[] = [];
  if (props.hint && !props.error) ids.push(hintId);
  if (props.error) ids.push(errorId);
  if (props.describedBy) ids.push(props.describedBy);
  return ids.length > 0 ? ids.join(" ") : undefined;
});
</script>

<template>
  <div class="flex min-w-0 flex-col gap-1">
    <label :for="id" class="text-label font-medium text-muted-foreground">{{ label }}</label>
    <input
      :id="id"
      :value="modelValue"
      :placeholder="placeholder"
      :disabled="disabled"
      :aria-invalid="error ? true : undefined"
      :aria-describedby="describedBy"
      class="h-8 w-full min-w-0 rounded-control border bg-background px-2.5 text-body text-foreground transition-colors duration-150 placeholder:text-muted-foreground/70 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:cursor-not-allowed disabled:opacity-45"
      :class="[
        mono ? 'font-mono' : '',
        error ? 'border-danger' : 'border-control-border',
      ]"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    />
    <p v-if="error" :id="errorId" class="text-caption text-danger" role="alert">{{ error }}</p>
    <p v-else-if="hint" :id="hintId" class="text-caption text-muted-foreground">{{ hint }}</p>
  </div>
</template>
