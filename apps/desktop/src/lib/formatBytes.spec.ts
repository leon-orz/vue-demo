import { describe, expect, it } from 'vitest'

import { formatBytes } from './formatBytes'

describe('formatBytes', () => {
  it('keeps byte values exact below one kibibyte and scales larger values', () => {
    expect(formatBytes(0n)).toBe('0 B')
    expect(formatBytes(1023n)).toBe('1023 B')
    expect(formatBytes(1024n)).toBe('1.0 KB')
    expect(formatBytes(1_048_576n)).toBe('1.0 MB')
    expect(formatBytes(1_099_511_627_776n)).toBe('1.0 TB')
  })
})
