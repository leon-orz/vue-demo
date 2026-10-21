import { describe, expect, it } from 'vitest'

import { formatDateTime } from './format'

describe('formatDateTime', () => {
  it('非法时间原样返回', () => {
    expect(formatDateTime('not-a-date')).toBe('not-a-date')
  })
})
