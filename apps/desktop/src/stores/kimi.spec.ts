import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { UsageRange, UsageSummary } from '@toogl/core-types'

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

function summaryOf(range: UsageRange, input: number): UsageSummary {
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
  mocks.kimiUsageSummary.mockImplementation((range: UsageRange) =>
    Promise.resolve(summaryOf(range, 100)),
  )
  mocks.kimiUsageByProject.mockResolvedValue([])
  mocks.kimiUsageByModel.mockResolvedValue([])
  mocks.kimiUsageSeries.mockImplementation((range: UsageRange) =>
    Promise.resolve({ range, points: [] }),
  )
  mocks.kimiBalanceLatest.mockResolvedValue(null)
  mocks.kimiAccountGet.mockResolvedValue(null)
  mocks.scanKimiLogs.mockResolvedValue({
    files_found: 1,
    files_rescanned: 0,
    files_failed: 0,
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

  it('加载失败时不抛异常、不置 initialized、错误进 loadError，且不启动后台扫描', async () => {
    mocks.kimiUsageSummary.mockRejectedValue({ kind: 'internal', message: '数据库锁定' })
    const store = useKimiStore()
    await store.init()
    expect(store.initialized).toBe(false)
    expect(store.loadError).toEqual({ kind: 'internal', message: '数据库锁定' })
    expect(mocks.scanKimiLogs).not.toHaveBeenCalled()
  })

  it('init 失败后可重试：修复后再次 init 成功并完成初始化', async () => {
    mocks.kimiUsageSummary.mockRejectedValueOnce(new Error('临时失败'))
    const store = useKimiStore()
    await store.init()
    expect(store.initialized).toBe(false)
    expect(store.loadError).not.toBeNull()

    await store.init()
    expect(store.loadError).toBeNull()
    expect(store.initialized).toBe(true)
    expect(store.todaySummary?.totals.input_other).toBe(100)
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    expect(mocks.scanKimiLogs).toHaveBeenCalledTimes(1)
  })

  it('retry 在已初始化时只重取聚合并清空错误', async () => {
    const store = useKimiStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))

    mocks.kimiUsageSummary.mockRejectedValueOnce(new Error('抖动'))
    await store.loadAggregates()
    expect(store.loadError?.message).toBe('抖动')

    await store.retry()
    expect(store.loadError).toBeNull()
    // 不重复触发初始化扫描
    expect(mocks.scanKimiLogs).toHaveBeenCalledTimes(1)
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

  it('快速切换时丢弃过期的旧响应，慢请求不覆盖新数据', async () => {
    const store = useKimiStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))

    // Week 响应慢（手动控制），Month 立即返回
    let resolveWeek!: (v: { range: string; points: never[] }) => void
    mocks.kimiUsageSeries.mockImplementation((range: UsageRange) => {
      if (range === 'Week') {
        return new Promise((resolve) => {
          resolveWeek = resolve
        })
      }
      return Promise.resolve({ range, points: [] })
    })

    const weekReq = store.setTrendRange('Week')
    const monthReq = store.setTrendRange('Month')
    await monthReq
    // Month 已落地，Week 的慢响应随后才返回，应被丢弃
    resolveWeek({ range: 'Week', points: [] })
    await weekReq

    expect(store.trendRange).toBe('Month')
    expect(store.series?.range).toBe('Month')
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
    expect(store.balanceError?.kind).toBe('internal')
    expect(store.balanceError?.message).toContain('尚未配置')
  })

  it('401 类失败被归类为 unauthorized', async () => {
    mocks.kimiBalanceRefresh.mockRejectedValue({ kind: 'unauthorized', message: 'token 无效' })
    const store = useKimiStore()
    await store.refreshBalance()
    expect(store.balanceError).toEqual({ kind: 'unauthorized', message: 'token 无效' })
  })
})
