<script setup lang="ts">
// 密集表格：可排序表头（带 aria-sort）、粘性表头、行悬停、行内操作插槽。
// 布局参考 WailBrew 的已安装列表：NAME / VERSION / SIZE / ACTIONS。
import { PhCaretDown, PhCaretUp } from "@phosphor-icons/vue";

import type { TableColumn, TableRow } from "../../types";

withDefaults(
  defineProps<{
    caption: string;
    columns: TableColumn[];
    rows: TableRow[];
    sortKey?: string | null;
    sortDir?: "asc" | "desc";
  }>(),
  { sortDir: "asc", sortKey: null },
);

const emit = defineEmits<{ sort: [key: string] }>();

function ariaSort(key: string, sortKey: string | null | undefined, dir: "asc" | "desc") {
  if (sortKey !== key) return "none" as const;
  return dir === "asc" ? ("ascending" as const) : ("descending" as const);
}
</script>

<template>
  <table class="w-full border-collapse text-body">
    <caption class="sr-only">{{ caption }}</caption>
    <thead>
      <tr>
        <th
          v-for="column in columns"
          :key="column.key"
          scope="col"
          class="sticky top-0 z-10 border-b border-border bg-surface px-3 py-2 text-left font-normal label-mini"
          :style="column.width ? { width: column.width } : undefined"
          :class="column.align === 'right' ? 'text-right' : ''"
          :aria-sort="column.sortable ? ariaSort(column.key, sortKey, sortDir) : undefined"
        >
          <button
            v-if="column.sortable"
            type="button"
            class="inline-flex cursor-pointer items-center gap-1 transition-colors duration-150 hover:text-foreground"
            :class="sortKey === column.key ? 'text-foreground' : ''"
            @click="emit('sort', column.key)"
          >
            {{ column.label }}
            <component
              :is="sortDir === 'asc' && sortKey === column.key ? PhCaretUp : PhCaretDown"
              :size="10"
              aria-hidden="true"
            />
          </button>
          <span v-else>{{ column.label }}</span>
        </th>
      </tr>
    </thead>
    <tbody>
      <tr
        v-for="row in rows"
        :key="row.id"
        class="border-b border-border transition-colors duration-150 last:border-b-0 hover:bg-muted"
      >
        <td
          v-for="column in columns"
          :key="column.key"
          class="px-3 py-2 align-middle"
          :class="[
            column.align === 'right' ? 'text-right tabular-nums' : '',
            column.mono ? 'font-mono text-label' : '',
          ]"
        >
          <slot name="cell" :row="row" :column="column">
            {{ row[column.key] }}
          </slot>
        </td>
      </tr>
      <tr v-if="rows.length === 0">
        <td :colspan="columns.length" class="border-b-0 px-0 py-0">
          <slot name="empty" />
        </td>
      </tr>
    </tbody>
  </table>
</template>
