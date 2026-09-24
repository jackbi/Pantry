<script setup lang="ts">
// 分段控件：同一页面里的同级视图切换（已安装 / 安装新包）。
//
// 刻意与 AppTag 区分开：标签是"可叠加的筛选"，这里是二选一。
// 两者外形相近会导致误读，所以这里用带底槽的分段控件，选中项有底色。
defineProps<{ tabs: { key: string; label: string }[]; current: string; label: string }>();
const emit = defineEmits<{ select: [key: string] }>();
</script>

<template>
  <div
    class="inline-flex w-max rounded-control border border-border bg-surface p-0.5"
    role="group"
    :aria-label="label"
  >
    <button
      v-for="tab in tabs"
      :key="tab.key"
      type="button"
      class="cursor-pointer rounded-[3px] px-3 py-1 text-label transition-colors duration-150"
      :class="
        current === tab.key
          ? 'bg-muted font-medium text-foreground'
          : 'text-muted-foreground hover:text-foreground'
      "
      :aria-pressed="current === tab.key"
      @click="emit('select', tab.key)"
    >
      {{ tab.label }}
    </button>
  </div>
</template>
