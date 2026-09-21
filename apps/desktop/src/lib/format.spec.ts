import { describe, expect, it } from 'vitest'

import { formatDateTime, formatMoney, formatPercent, formatTokens, inputTotal } from './format'

describe('formatTokens', () => {
  it('千级以下原样显示', () => {
    expect(formatTokens(0)).toBe('0')
    expect(formatTokens(999)).toBe('999')
  })

  it('万级用「万」并去掉多余的 .0', () => {
    expect(formatTokens(10000)).toBe('1万')
    expect(formatTokens(572340)).toBe('57.2万')
  })

  it('亿级用「亿」', () => {
    expect(formatTokens(123456789)).toBe('1.2亿')
  })
})

describe('formatPercent', () => {
  it('null 显示占位符', () => {
    expect(formatPercent(null)).toBe('—')
  })

  it('保留一位小数', () => {
    expect(formatPercent(0.9734)).toBe('97.3%')
    expect(formatPercent(0)).toBe('0.0%')
  })
})

describe('formatMoney / formatDateTime / inputTotal', () => {
  it('金额两位小数', () => {
    expect(formatMoney(10.5)).toBe('¥10.50')
  })

  it('非法时间原样返回', () => {
    expect(formatDateTime('not-a-date')).toBe('not-a-date')
  })

  it('输入总量 = 普通输入 + 缓存读 + 缓存写', () => {
    expect(inputTotal({ input_other: 100, cache_read: 900, cache_creation: 50 })).toBe(1050)
  })
})
