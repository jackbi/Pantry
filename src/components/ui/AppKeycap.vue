<script setup lang="ts">
// 键帽：快捷键提示。参考 npmx 的 `jump to…  Ctrl+K`。
defineProps<{ keys: string }>();
</script>

<template>
  <span class="keycap" aria-hidden="true">{{ keys }}</span>
</template>
