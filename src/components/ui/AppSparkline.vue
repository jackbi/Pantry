<script setup lang="ts">
// 近 7 天下载量趋势线。npm 官网只给数字，这里补一条极简折线。
// 无填充、无渐变、无坐标轴——数值本身已经用文本给出，图形只做趋势辅助。
import { computed } from "vue";

const props = defineProps<{
  values: number[];
  /** 读屏用的文字描述，例如「近 7 天下载趋势，最低 1，最高 9」 */
  label: string;
}>();

const WIDTH = 60;
const HEIGHT = 16;
// 上下各留 1px，避免折线在最高/最低点被裁掉
const INSET = 1;

const points = computed(() => {
  const values = props.values;
  if (values.length < 2) return "";
  const max = Math.max(...values);
  const min = Math.min(...values);
  const span = max - min;
  const usable = HEIGHT - INSET * 2;
  return values
    .map((value, index) => {
      const x = (index / (values.length - 1)) * WIDTH;
      // 数值全平时画中线，不要贴底
      const ratio = span === 0 ? 0.5 : (value - min) / span;
      const y = INSET + (1 - ratio) * usable;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
});
</script>

<template>
  <svg
    v-if="points"
    :viewBox="`0 0 ${WIDTH} ${HEIGHT}`"
    preserveAspectRatio="none"
    class="h-4 w-16 shrink-0 text-accent"
    fill="none"
    role="img"
    :aria-label="label"
  >
    <polyline
      :points="points"
      stroke="currentColor"
      stroke-width="1.25"
      stroke-linecap="round"
      stroke-linejoin="round"
      vector-effect="non-scaling-stroke"
    />
  </svg>
</template>
