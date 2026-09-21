/**
 * Tauri 命令错误的统一适配。
 *
 * 后端约定：invoke 失败时 reject 结构化对象 { kind, message }（Rust 侧 CommandError，
 * 见 crates/core/src/models/error.rs）；类型 re-export 自 ts-rs 生成物。
 */
import type { CommandError } from '@toogl/core-types'

export type { CommandError }

/** 错误类别：unauthorized 引导去设置页；network 可重试；其余按 message 展示 */
export type CommandErrorKind = 'unauthorized' | 'network' | 'not_found' | 'internal' | 'unavailable'

const KINDS: readonly CommandErrorKind[] = [
  'unauthorized',
  'network',
  'not_found',
  'internal',
  'unavailable',
]

/** 把 invoke 抛出的任意值规整为 CommandError；非结构化值归为 internal */
export function toCommandError(err: unknown): CommandError {
  if (typeof err === 'object' && err !== null) {
    const { kind, message } = err as { kind?: unknown; message?: unknown }
    if (typeof message === 'string') {
      return {
        kind: KINDS.includes(kind as CommandErrorKind) ? (kind as CommandErrorKind) : 'internal',
        message,
      }
    }
  }
  if (err instanceof Error) return { kind: 'internal', message: err.message }
  return { kind: 'internal', message: String(err) }
}
