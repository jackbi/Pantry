/** 浏览器里预览时没有 Tauri 注入的 IPC，用它做降级判断。 */
export const isTauri = "__TAURI_INTERNALS__" in window;
