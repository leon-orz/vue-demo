import { describe, expect, it } from 'vitest'

import { toCommandError } from './errors'

describe('toCommandError', () => {
  it('结构化对象原样保留 kind 与 message', () => {
    expect(toCommandError({ kind: 'unauthorized', message: 'token 无效' })).toEqual({
      kind: 'unauthorized',
      message: 'token 无效',
    })
    expect(toCommandError({ kind: 'network', message: '连接超时' }).kind).toBe('network')
  })

  it('未知 kind 归一为 internal，保留 message', () => {
    expect(toCommandError({ kind: 'weird', message: 'x' })).toEqual({
      kind: 'internal',
      message: 'x',
    })
  })

  it('Error 实例取 message，归 internal', () => {
    expect(toCommandError(new Error('boom'))).toEqual({ kind: 'internal', message: 'boom' })
  })

  it('字符串与其他原始值转 message，归 internal', () => {
    expect(toCommandError('尚未配置')).toEqual({ kind: 'internal', message: '尚未配置' })
    expect(toCommandError(null).kind).toBe('internal')
    expect(toCommandError(undefined).kind).toBe('internal')
  })
})
