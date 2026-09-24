import { onScopeDispose, watch, type Ref } from "vue";

/**
 * 弹层打开时锁住主滚动容器。
 *
 * 页面滚动发生在 <main> 上而不是 body，所以只设 body 的 overflow 没用。
 * 不锁的话弹层背后还能滚动，视觉上就是两根滚动条同时存在。
 *
 * 用引用计数 + 组件卸载时归还：命令面板与页面里的弹层可能同时开着，
 * 先关的那个不能把另一个的锁解掉；弹层开着被卸载（比如跳页）也得把锁还回去。
 */
let locks = 0;

export function useScrollLock(open: Ref<boolean>) {
  let held = false;

  function sync(locked: boolean) {
    if (locked === held) return;
    held = locked;
    locks = Math.max(0, locks + (locked ? 1 : -1));
    const main = document.getElementById("main");
    if (main) main.style.overflowY = locks > 0 ? "hidden" : "";
  }

  watch(open, (value) => sync(value), { immediate: true });
  onScopeDispose(() => sync(false));
}
