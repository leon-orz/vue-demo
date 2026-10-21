/** 数字与时间的展示格式化（纯函数，供单测直接覆盖） */
import type { DelayStatus } from '@toogl/core-types'

/** RFC 3339 时间戳转本地短格式（MM-DD HH:mm:ss） */
export function formatDateTime(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const pad = (v: number) => String(v).padStart(2, '0')
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

/** 节点延迟展示：未测/超时用文字，已测带毫秒 */
export function formatDelay(delayMs: number | null, status: DelayStatus): string {
  if (status === 'untested') return '未测'
  if (status === 'timeout') return '超时'
  return delayMs === null ? '—' : `${delayMs} ms`
}

/** 节点延迟着色：<200ms 绿，<800ms 黄，其余红；未测/超时灰 */
export function delayToneClass(delayMs: number | null, status: DelayStatus): string {
  if (status !== 'delayed' || delayMs === null) return 'text-muted-foreground'
  if (delayMs < 200) return 'text-green-600 dark:text-green-400'
  if (delayMs < 800) return 'text-yellow-600 dark:text-yellow-400'
  return 'text-red-600 dark:text-red-400'
}
