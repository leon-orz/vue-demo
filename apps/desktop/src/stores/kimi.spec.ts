import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { KimiUsageRange, KimiUsageSummary } from '@toogl/core-types'

// 与后端交互的 api 层整体 mock，只验证 store 的聚合/切换逻辑
const mocks = vi.hoisted(() => ({
  scanKimiLogs: vi.fn(),
  kimiUsageSummary: vi.fn(),
  kimiUsageByProject: vi.fn(),
  kimiUsageByModel: vi.fn(),
  kimiUsageSeries: vi.fn(),
  kimiBalanceLatest: vi.fn(),
  kimiBalanceRefresh: vi.fn(),
  kimiAccountGet: vi.fn(),
  kimiAccountSet: vi.fn(),
}))
vi.mock('@/api/kimi', () => mocks)

import { useKimiStore } from './kimi'

function summaryOf(range: KimiUsageRange, input: number): KimiUsageSummary {
  return {
    range,
    totals: {
      input_other: input,
      output: 10,
      cache_read: 900,
      cache_creation: 0,
      cache_hit_rate: 0.9,
      estimated_cost: 0,
    },
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  mocks.kimiUsageSummary.mockImplementation((range: KimiUsageRange) =>
    Promise.resolve(summaryOf(range, 100)),
  )
  mocks.kimiUsageByProject.mockResolvedValue([])
  mocks.kimiUsageByModel.mockResolvedValue([])
  mocks.kimiUsageSeries.mockImplementation((range: KimiUsageRange) =>
    Promise.resolve({ range, points: [] }),
  )
  mocks.kimiBalanceLatest.mockResolvedValue(null)
  mocks.kimiAccountGet.mockResolvedValue(null)
  mocks.scanKimiLogs.mockResolvedValue({
    files_found: 1,
    files_rescanned: 0,
    events_added: 3,
    partial_lines_skipped: 0,
    bad_lines: 0,
  })
})

describe('kimi store 聚合加载', () => {
  it('init 先读缓存聚合，再后台扫描并刷新，全程不抛出', async () => {
    const store = useKimiStore()
    await store.init('D:/logs')
    // init 返回时缓存数据已就位
    expect(store.todaySummary?.totals.input_other).toBe(100)
    expect(store.monthSummary?.range).toBe('Month')
    // 后台扫描：传入的覆盖目录透传给后端
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    expect(mocks.scanKimiLogs).toHaveBeenCalledWith('D:/logs')
    expect(store.lastScanAt).not.toBeNull()
    expect(store.lastScanReport?.events_added).toBe(3)
    // 扫描完成后聚合被重新拉取
    expect(mocks.kimiUsageSummary.mock.calls.length).toBeGreaterThanOrEqual(6)
  })

  it('重复 init 只执行一次', async () => {
    const store = useKimiStore()
    await store.init()
    await store.init()
    expect(mocks.scanKimiLogs).toHaveBeenCalledTimes(1)
  })

  it('扫描失败时错误进 scanError，不影响已加载数据', async () => {
    mocks.scanKimiLogs.mockRejectedValue(new Error('目录不存在'))
    const store = useKimiStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    expect(store.scanError).toContain('目录不存在')
    expect(store.todaySummary).not.toBeNull()
  })
})

describe('趋势范围切换', () => {
  it('setTrendRange 重新拉取序列与分组', async () => {
    const store = useKimiStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    vi.clearAllMocks()

    await store.setTrendRange('Week')
    expect(store.trendRange).toBe('Week')
    expect(mocks.kimiUsageSeries).toHaveBeenCalledWith('Week')
    expect(mocks.kimiUsageByProject).toHaveBeenCalledWith('Week')
    expect(mocks.kimiUsageByModel).toHaveBeenCalledWith('Week')
    expect(store.series?.range).toBe('Week')
  })
})

describe('余额', () => {
  it('刷新成功更新 balance 并清空错误', async () => {
    mocks.kimiBalanceRefresh.mockResolvedValue({
      available: 10,
      voucher: 2,
      cash: 8,
      fetched_at: '2026-01-01T00:00:00Z',
    })
    const store = useKimiStore()
    await store.refreshBalance()
    expect(store.balance?.available).toBe(10)
    expect(store.balanceError).toBeNull()
  })

  it('未配置 key 时刷新失败，错误文案进 balanceError', async () => {
    mocks.kimiBalanceRefresh.mockRejectedValue('尚未配置 Kimi 开放平台账户')
    const store = useKimiStore()
    await store.refreshBalance()
    expect(store.balance).toBeNull()
    expect(store.balanceError).toContain('尚未配置')
  })
})
