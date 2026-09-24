<script setup lang="ts">
// 命令输出面板：等宽、区分 stdout/stderr/元信息、空态提示、aria-live 播报。
import { nextTick, onMounted, ref, watch } from "vue";
import { PhTerminal } from "@phosphor-icons/vue";

type LogLine = { stream: string; line: string };

const props = defineProps<{ lines: LogLine[]; placeholder?: string; height?: string }>();

const box = ref<HTMLElement | null>(null);
// 用户主动往上翻时不要强制拉回底部
const follow = ref(true);

function onScroll() {
  const element = box.value;
  if (!element) return;
  follow.value = element.scrollHeight - element.scrollTop - element.clientHeight < 24;
}

watch(
  () => props.lines.length,
  async () => {
    if (!follow.value) return;
    await nextTick();
    const element = box.value;
    if (element) element.scrollTop = element.scrollHeight;
  },
);

onMounted(() => {
  const element = box.value;
  if (element) element.scrollTop = element.scrollHeight;
});
</script>

<template>
  <div
    ref="box"
    class="overflow-auto rounded-control border border-border bg-muted p-3"
    :style="{ height: height ?? '15rem' }"
    role="log"
    aria-live="polite"
    aria-relevant="additions"
    tabindex="0"
    @scroll="onScroll"
  >
    <p v-if="lines.length === 0" class="flex items-center gap-2 font-mono text-label text-muted-foreground">
      <PhTerminal :size="14" aria-hidden="true" />
      {{ placeholder ?? "还没有输出。执行一个命令后，日志会实时出现在这里。" }}
    </p>
    <pre v-else class="font-mono text-label leading-relaxed wrap-token whitespace-pre-wrap"><span
      v-for="(entry, index) in lines"
      :key="index"
      :class="
        entry.stream === 'stderr'
          ? 'text-danger'
          : entry.stream === 'meta'
            ? 'text-primary'
            : 'text-foreground'
      "
    >{{ entry.line }}
</span></pre>
  </div>
</template>
