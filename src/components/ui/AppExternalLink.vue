<script setup lang="ts">
// 外链：在应用内的独立窗口里打开主页 / 仓库。
//
// 细节说明：详情里的地址以前是纯文本，用户得自己复制到浏览器；现在点一下就开，
// 而且开在应用自己的窗口里（后端 open_external_url），不抢系统浏览器的焦点。
// 打开失败的原因（地址不是 http/https、窗口建不起来）通过 failed 交回调用方，
// 由页面统一报错——这个组件不自己吞掉错误。
import { PhArrowSquareOut } from "@phosphor-icons/vue";
import { invoke } from "@tauri-apps/api/core";

const props = defineProps<{
  url: string;
  /** 链接窗口的标题，一般写成「包名 · 主页」 */
  windowTitle?: string;
}>();

const emit = defineEmits<{ failed: [message: string] }>();

async function open() {
  try {
    await invoke<string>("open_external_url", {
      url: props.url,
      title: props.windowTitle ?? null,
    });
  } catch (error) {
    emit("failed", error instanceof Error ? error.message : String(error));
  }
}
</script>

<template>
  <button
    type="button"
    class="inline-flex max-w-full cursor-pointer items-start gap-1.5 rounded-control text-left font-mono text-label text-accent transition-colors duration-150 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring active:opacity-80"
    :title="url"
    :aria-label="`在应用内打开 ${url}`"
    @click="open"
  >
    <PhArrowSquareOut :size="13" class="mt-0.5 shrink-0" aria-hidden="true" />
    <span class="wrap-token">{{ url }}</span>
  </button>
</template>
