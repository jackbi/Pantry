/**
 * 文案里是否含中日韩字符。
 *
 * JetBrains Mono 没有中文字形，MASTER.md 因此规定：等宽只用于拉丁与数字，
 * 中文标签套 `font-mono` 属于反模式（对中文等于没生效，只会回退到系统字体）。
 * 共享组件里用这个判断决定要不要加 `font-mono`。
 */
export function hasCjk(value: string): boolean {
  return /[\u3000-\u303f\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uff00-\uffef]/.test(value);
}

/** 拉丁 / 数字文案才加等宽 */
export function tokenFont(value: string): string {
  return hasCjk(value) ? "" : "font-mono";
}
