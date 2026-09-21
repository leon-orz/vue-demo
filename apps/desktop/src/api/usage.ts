// M1 Kimi 统计的前端封装已迁至 ./kimi.ts（S1-2）。
// 本文件保留给 M2 我的 API 平台（S2-3）：届时对应 src-tauri 的 gateway_* 命令，
// invoke 参数与返回类型一律使用 @toogl/core-types 中由 ts-rs 生成的类型，禁止手写。
// 注意：本文件暂无调用方，S2-3 网关落地前勿引用。
import type { UsageSnapshot } from '@toogl/core-types'

export type { UsageSnapshot }

export async function refreshUsage(_platformId?: number): Promise<UsageSnapshot[]> {
  throw new Error('网关用量命令将在 S2-3 落地')
}
