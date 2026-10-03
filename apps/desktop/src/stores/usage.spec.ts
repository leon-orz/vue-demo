import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type {
  Platform,
  PlatformScanReport,
  QuotaSnapshot,
  UsageRange,
  UsageSeries,
  UsageSummary,
} from '@toogl/core-types'

// 与后端交互的 api 层整体 mock，只验证 store 的聚合/切换逻辑
const mocks = vi.hoisted(() => ({
  scanUsageLogs: vi.fn(),
  usageSummary: vi.fn(),
  usageByProject: vi.fn(),
  usageByModel: vi.fn(),
  usageSeries: vi.fn(),
  quotaRefresh: vi.fn(),
  quotaLatest: vi.fn(),
  platformAccountGet: vi.fn(),
  platformAccountSet: vi.fn(),
}))
vi.mock('@/api/usage', () => mocks)

import { parseKimiQuotaDetail, useUsageStore } from './usage'

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

function scanReportOf(
  platform: Platform,
  events: number,
  error: string | null = null,
): PlatformScanReport {
  return {
    platform,
    report: error
      ? null
      : {
          files_found: 1,
          files_rescanned: 0,
          files_failed: 0,
          events_added: events,
          partial_lines_skipped: 0,
          bad_lines: 0,
        },
    error,
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  mocks.usageSummary.mockImplementation((_platform: string, range: UsageRange) =>
    Promise.resolve(summaryOf(range, 100)),
  )
  mocks.usageByProject.mockResolvedValue([])
  mocks.usageByModel.mockResolvedValue([])
  mocks.usageSeries.mockImplementation((_platform: string, range: UsageRange) =>
    Promise.resolve({ range, points: [] }),
  )
  mocks.quotaLatest.mockResolvedValue(null)
  mocks.platformAccountGet.mockResolvedValue(null)
  mocks.scanUsageLogs.mockResolvedValue([scanReportOf('kimi', 3)])
})

describe('usage store 聚合加载', () => {
  it('init 先读缓存聚合，再后台扫描并刷新，全程不抛出', async () => {
    const store = useUsageStore()
    await store.init('D:/logs')
    // init 返回时缓存数据已就位（默认过滤为 kimi 单平台）
    expect(store.todaySummary?.totals.input_other).toBe(100)
    expect(store.monthSummary?.range).toBe('Month')
    // 后台扫描：platform 不传，覆盖目录透传给后端
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    expect(mocks.scanUsageLogs).toHaveBeenCalledWith(undefined, 'D:/logs')
    expect(store.lastScanAt).not.toBeNull()
    expect(store.lastScanEventsAdded).toBe(3)
    // 扫描完成后聚合被重新拉取（init 一轮 3 条 summary + 扫描后一轮 3 条）
    expect(mocks.usageSummary.mock.calls.length).toBeGreaterThanOrEqual(6)
  })

  it('重复 init 只执行一次', async () => {
    const store = useUsageStore()
    await store.init()
    await store.init()
    expect(mocks.scanUsageLogs).toHaveBeenCalledTimes(1)
  })

  it('扫描命令失败时合成失败报告，错误经 lastScanFailures 展示，不影响已加载数据', async () => {
    mocks.scanUsageLogs.mockRejectedValue(new Error('数据库锁定'))
    const store = useUsageStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    expect(store.lastScanFailures.length).toBeGreaterThan(0)
    expect(store.lastScanFailures.every((r) => r.error === '数据库锁定')).toBe(true)
    expect(store.todaySummary).not.toBeNull()
  })

  it('扫描失败不算整体失败，只记录到分平台报告', async () => {
    mocks.scanUsageLogs.mockResolvedValue([scanReportOf('kimi', 0, 'Kimi 日志目录不存在')])
    const store = useUsageStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    expect(store.lastScanFailures).toHaveLength(1)
    expect(store.lastScanFailures[0].platform).toBe('kimi')
  })

  it('加载失败时不抛异常、不置 initialized、错误进 loadError，且不启动后台扫描', async () => {
    mocks.usageSummary.mockRejectedValue({ kind: 'internal', message: '数据库锁定' })
    const store = useUsageStore()
    await store.init()
    expect(store.initialized).toBe(false)
    expect(store.loadError).toEqual({ kind: 'internal', message: '数据库锁定' })
    expect(mocks.scanUsageLogs).not.toHaveBeenCalled()
  })

  it('init 失败后可重试：修复后再次 init 成功并完成初始化', async () => {
    mocks.usageSummary.mockRejectedValueOnce(new Error('临时失败'))
    const store = useUsageStore()
    await store.init()
    expect(store.initialized).toBe(false)
    expect(store.loadError).not.toBeNull()

    await store.init()
    expect(store.loadError).toBeNull()
    expect(store.initialized).toBe(true)
    expect(store.todaySummary?.totals.input_other).toBe(100)
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    expect(mocks.scanUsageLogs).toHaveBeenCalledTimes(1)
  })

  it('retry 在已初始化时只重取聚合并清空错误', async () => {
    const store = useUsageStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))

    mocks.usageSummary.mockRejectedValueOnce(new Error('抖动'))
    await store.loadAggregates()
    expect(store.loadError?.message).toBe('抖动')

    await store.retry()
    expect(store.loadError).toBeNull()
    // 不重复触发初始化扫描
    expect(mocks.scanUsageLogs).toHaveBeenCalledTimes(1)
  })
})

describe('平台过滤切换', () => {
  it('setActiveFilter 按新过滤重取聚合，summary 请求带平台参数', async () => {
    const store = useUsageStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    vi.clearAllMocks()

    await store.setActiveFilter('all')
    expect(store.activeFilter).toBe('all')
    expect(mocks.usageSummary).toHaveBeenCalledWith('all', 'Day')
    expect(mocks.usageSeries).toHaveBeenCalledWith('all', 'Day')
    expect(mocks.usageByProject).toHaveBeenCalledWith('all', 'Day')
  })

  it('切换到相同过滤不重复请求', async () => {
    const store = useUsageStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    vi.clearAllMocks()

    await store.setActiveFilter('kimi')
    expect(mocks.usageSummary).not.toHaveBeenCalled()
  })
})

describe('趋势范围切换', () => {
  it('setTrendRange 重新拉取序列与分组', async () => {
    const store = useUsageStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))
    vi.clearAllMocks()

    await store.setTrendRange('Week')
    expect(store.trendRange).toBe('Week')
    expect(mocks.usageSeries).toHaveBeenCalledWith('kimi', 'Week')
    expect(mocks.usageByProject).toHaveBeenCalledWith('kimi', 'Week')
    expect(mocks.usageByModel).toHaveBeenCalledWith('kimi', 'Week')
    expect(store.series?.range).toBe('Week')
  })

  it('快速切换时丢弃过期的旧响应，慢请求不覆盖新数据', async () => {
    const store = useUsageStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))

    // Week 响应慢（手动控制），Month 立即返回
    let resolveWeek!: (v: { range: string; points: never[] }) => void
    mocks.usageSeries.mockImplementation((_platform: string, range: UsageRange) => {
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

describe('聚合数据 stale-write 竞态（loadAggregates 与 setTrendRange 共用请求序号）', () => {
  it('切平台过滤后，旧 filter 的在途趋势响应被丢弃，不覆盖新过滤数据', async () => {
    const store = useUsageStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))

    // 旧 filter（kimi）的 Week 序列响应慢（手动控制），新 filter 的请求立即返回；
    // byProject 用 key 标记来源过滤，便于区分新旧数据
    let resolveStale!: (v: UsageSeries) => void
    mocks.usageSeries.mockImplementation((platform: string, range: UsageRange) => {
      if (platform === 'kimi' && range === 'Week') {
        return new Promise((resolve) => {
          resolveStale = resolve
        })
      }
      return Promise.resolve({ range, points: [] })
    })
    mocks.usageByProject.mockImplementation((platform: string) =>
      Promise.resolve([
        {
          key: `proj-${platform}`,
          totals: {
            input_other: 0,
            output: 0,
            cache_read: 0,
            cache_creation: 0,
            cache_hit_rate: 0,
            estimated_cost: 0,
          },
        },
      ]),
    )

    const weekReq = store.setTrendRange('Week')
    const filterReq = store.setActiveFilter('all')
    await filterReq
    expect(store.byProject[0]?.key).toBe('proj-all')

    // 旧 filter 的慢响应随后落地，应被丢弃
    resolveStale({ range: 'Week', points: [] })
    await weekReq
    expect(store.activeFilter).toBe('all')
    expect(store.byProject[0]?.key).toBe('proj-all')
    expect(store.loadError).toBeNull()
  })

  it('切趋势范围后，旧范围的在途聚合响应被丢弃，不覆盖新范围数据', async () => {
    const store = useUsageStore()
    await store.init()
    await vi.waitFor(() => expect(store.scanning).toBe(false))

    // 切过滤触发的 loadAggregates（all + 当前 Day 范围）序列响应慢，随后的切范围请求立即返回
    let resolveStale!: (v: UsageSeries) => void
    mocks.usageSeries.mockImplementation((platform: string, range: UsageRange) => {
      if (platform === 'all' && range === 'Day') {
        return new Promise((resolve) => {
          resolveStale = resolve
        })
      }
      return Promise.resolve({ range, points: [] })
    })

    const filterReq = store.setActiveFilter('all')
    const monthReq = store.setTrendRange('Month')
    await monthReq
    expect(store.series?.range).toBe('Month')

    // 旧范围的慢聚合响应随后落地，应被丢弃
    resolveStale({ range: 'Day', points: [] })
    await filterReq
    expect(store.trendRange).toBe('Month')
    expect(store.series?.range).toBe('Month')
    expect(store.loadError).toBeNull()
  })
})

describe('配额刷新', () => {
  it('刷新成功更新快照并清空错误', async () => {
    const snapshot: QuotaSnapshot = {
      platform: 'kimi',
      total: null,
      used: null,
      remaining: 10,
      detail_json:
        '{"sources":{"balance":{"available":10,"voucher":2,"cash":8,"currency":"cny"},"coding_usages":null,"coding_usages_error":"接口 404"}}',
      fetched_at: '2026-01-01T00:00:00Z',
    }
    mocks.quotaRefresh.mockResolvedValue(snapshot)
    const store = useUsageStore()
    await store.refreshQuota('kimi')
    expect(store.quotas.kimi?.remaining).toBe(10)
    expect(store.kimiQuotaDetail?.balance?.voucher).toBe(2)
    expect(store.kimiQuotaDetail?.codingError).toBe('接口 404')
    expect(store.quotaErrors.kimi ?? null).toBeNull()
    expect(store.quotaLoading.kimi).toBe(false)
  })

  it('未配置 key 时刷新失败，错误文案进 quotaErrors', async () => {
    mocks.quotaRefresh.mockRejectedValue('尚未配置 kimi 平台账户')
    const store = useUsageStore()
    await store.refreshQuota('kimi')
    expect(store.quotas.kimi ?? null).toBeNull()
    expect(store.quotaErrors.kimi?.kind).toBe('internal')
    expect(store.quotaErrors.kimi?.message).toContain('尚未配置')
  })

  it('401 类失败被归类为 unauthorized', async () => {
    mocks.quotaRefresh.mockRejectedValue({ kind: 'unauthorized', message: 'token 无效' })
    const store = useUsageStore()
    await store.refreshQuota('kimi')
    expect(store.quotaErrors.kimi).toEqual({ kind: 'unauthorized', message: 'token 无效' })
  })
})

describe('配额 detail_json 解析', () => {
  const kimiSnapshot: QuotaSnapshot = {
    platform: 'kimi',
    total: 1000,
    used: 400,
    remaining: 600,
    detail_json:
      '{"sources":{"balance":{"available":10.5,"voucher":2,"cash":8.5,"currency":"cny"},"coding_usages":{"month_total":1000,"month_used":400,"month_remaining":600,"window_used_percent":37.5,"window_remaining_percent":62.5,"window_reset_at":"2026-01-01T05:00:00Z"},"coding_usages_error":null}}',
    fetched_at: '2026-01-01T00:00:00Z',
  }

  it('kimi：余额分项与 coding 月度池/窗口都解析出来', () => {
    const detail = parseKimiQuotaDetail(kimiSnapshot)
    expect(detail?.balance?.available).toBe(10.5)
    expect(detail?.coding?.month_remaining).toBe(600)
    expect(detail?.coding?.window_used_percent).toBe(37.5)
    expect(detail?.codingError).toBeNull()
  })

  it('非法 JSON / 空快照返回 null，不抛出', () => {
    expect(parseKimiQuotaDetail(null)).toBeNull()
    expect(
      parseKimiQuotaDetail({
        platform: 'kimi',
        total: null,
        used: null,
        remaining: null,
        detail_json: '{oops',
        fetched_at: '2026-01-01T00:00:00Z',
      }),
    ).toBeNull()
  })
})
