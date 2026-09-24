/**
 * 计数展示：中文千分位。
 *
 * 空值给破折号而不是 0——"没有数据"和"真的是 0"在界面上是两回事。
 * 三个视图都要用，之前各写了一份。
 */
export function formatNumber(value: number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  return new Intl.NumberFormat("zh-CN").format(value);
}
