import { ref } from "vue";

export type ThemeMode = "light" | "dark" | "system";

const STORAGE_KEY = "public-store:theme";
const query = window.matchMedia("(prefers-color-scheme: dark)");

function readStoredMode(): ThemeMode {
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored === "light" || stored === "dark" || stored === "system" ? stored : "system";
}

const mode = ref<ThemeMode>(readStoredMode());

function isDark(): boolean {
  return mode.value === "system" ? query.matches : mode.value === "dark";
}

async function syncWindowTheme(dark: boolean) {
  // 让窗口标题栏跟随主题；非 Tauri 环境（浏览器预览）直接跳过
  if (!("__TAURI_INTERNALS__" in window)) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().setTheme(dark ? "dark" : "light");
  } catch {
    // 窗口主题同步失败不影响页面渲染，静默降级
  }
}

function apply() {
  const dark = isDark();
  document.documentElement.classList.toggle("dark", dark);
  void syncWindowTheme(dark);
}

/** 在应用挂载前调用，避免首帧白闪。 */
export function initTheme() {
  apply();
  query.addEventListener("change", () => {
    if (mode.value === "system") apply();
  });
}

export function useTheme() {
  function setMode(next: ThemeMode) {
    mode.value = next;
    localStorage.setItem(STORAGE_KEY, next);
    apply();
  }

  return { mode, setMode };
}
