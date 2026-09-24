import { ref } from "vue";

import type { MarketSettings, RequestOptions } from "../types";

const STORAGE_KEY = "public-store:settings";

/**
 * 空字符串代表「用内置默认」，不在这里写死地址：
 * 默认源由后端 `market_defaults` 提供，改一处即可，两边不会漂移。
 */
const EMPTY: MarketSettings = { registry: "", proxy: "", insecure: false };

function readStored(): MarketSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...EMPTY };
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    return {
      registry: typeof parsed.registry === "string" ? parsed.registry : "",
      proxy: typeof parsed.proxy === "string" ? parsed.proxy : "",
      insecure: parsed.insecure === true,
    };
  } catch {
    // 存储被写坏时退回默认值，不能让应用起不来
    return { ...EMPTY };
  }
}

// 模块级单例：设置页保存后，其它页面下一次请求就用到新值
const settings = ref<MarketSettings>(readStored());

export function useSettings() {
  function save(next: MarketSettings) {
    settings.value = {
      registry: next.registry.trim(),
      proxy: next.proxy.trim(),
      insecure: next.insecure,
    };
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(settings.value));
    } catch {
      // 隐私模式等写入失败时不影响本次会话
    }
  }

  /** 传给 market / probe 类命令：空值转 null，由后端回落到内置默认或自动探测 */
  function requestOptions(): RequestOptions {
    return {
      registry: settings.value.registry || null,
      proxy: settings.value.proxy || null,
      insecure: settings.value.insecure,
    };
  }

  return { settings, save, requestOptions };
}
