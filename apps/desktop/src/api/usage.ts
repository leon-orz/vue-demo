import type { UsageSnapshot } from '@toogl/core-types'

export type { UsageSnapshot }

// TODO(S1-2)：对应 src-tauri 的 refresh_usage / usage_history 命令
// invoke 参数与返回类型一律使用 @toogl/core-types 中由 ts-rs 生成的类型，禁止手写
export async function refreshUsage(_platformId?: number): Promise<UsageSnapshot[]> {
  throw new Error('refresh_usage 命令将在 S1-2 落地')
}
