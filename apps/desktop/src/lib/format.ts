/** 数字与时间的展示格式化（纯函数，供单测直接覆盖） */

/** token 数量：>=1 亿用「亿」，>=1 万用「万」，其余原样千分位 */
export function formatTokens(n: number): string {
  if (n >= 1e8) return `${trimFixed(n / 1e8)}亿`
  if (n >= 1e4) return `${trimFixed(n / 1e4)}万`
  return n.toLocaleString('zh-CN')
}

/** 缓存命中率：null（无输入）显示占位符，否则保留一位小数 */
export function formatPercent(rate: number | null): string {
  if (rate === null) return '—'
  return `${(rate * 100).toFixed(1)}%`
}

/** 人民币金额，两位小数 */
export function formatMoney(n: number): string {
  return `¥${n.toFixed(2)}`
}

/** RFC 3339 时间戳转本地短格式（MM-DD HH:mm:ss） */
export function formatDateTime(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const pad = (v: number) => String(v).padStart(2, '0')
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

/** 输入侧总量（普通输入 + 缓存读 + 缓存写） */
export function inputTotal(t: {
  input_other: number
  cache_read: number
  cache_creation: number
}): number {
  return t.input_other + t.cache_read + t.cache_creation
}

/** 去掉小数尾部多余的 0（57.0万 → 57万，57.25万 → 57.3万） */
function trimFixed(v: number): string {
  return v.toFixed(1).replace(/\.0$/, '')
}
