<script setup lang="ts">
// 外链：用系统默认浏览器打开主页 / 仓库。
//
// 细节说明：详情里的地址以前是纯文本，用户得自己复制到浏览器；现在点一下就开，
// 开在系统浏览器里（后端 open_external_url）——那里有用户自己的登录态、代理与扩展，
// 需要 VPN 或需要登录的站点才打得开；应用内自建窗口这两样都没有。
// 打开失败的原因（地址不是 http/https、唤起浏览器失败）通过 failed 交回调用方，
// 由页面统一报错——这个组件不自己吞掉错误。
import { PhArrowSquareOut } from "@phosphor-icons/vue";
import { invoke } from "@tauri-apps/api/core";

const props = defineProps<{ url: string }>();

const emit = defineEmits<{ failed: [message: string] }>();

async function open() {
  try {
    await invoke<string>("open_external_url", { url: props.url });
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
    :aria-label="`在浏览器中打开 ${url}`"
    @click="open"
  >
    <PhArrowSquareOut :size="13" class="mt-0.5 shrink-0" aria-hidden="true" />
    <span class="wrap-token">{{ url }}</span>
  </button>
</template>
