import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { toCommandError, type CommandError } from '@/api/errors'
import * as api from '@/api/usage'
import type {
  Platform,
  PlatformAccountInfo,
  PlatformAccountInput,
  PlatformFilter,
  PlatformScanReport,
  QuotaSnapshot,
  UsageGroup,
  UsageRange,
  UsageSeries,
  UsageSummary,
} from '@toogl/core-types'

/** 趋势图可选范围（Last5Hours 不参与趋势切换） */
export type TrendRange = Exclude<UsageRange, 'Last5Hours'>

/** 有本地日志的平台（平台 Tab / 总览卡片墙顺序） */
export const USAGE_PLATFORMS: readonly Platform[] = ['kimi', 'mimo', 'omp', 'deepseek']

/** 有配额/余额数据源的平台；omp 仅本地日志统计，无配额卡 */
export const QUOTA_PLATFORMS: readonly Platform[] = ['kimi', 'mimo', 'deepseek']

export const PLATFORM_LABELS: Record<Platform, string> = {
  kimi: 'Kimi',
  mimo: 'MiMo',
  omp: 'omp',
  deepseek: 'DeepSeek',
}

// ---- 配额快照 detail_json 解析（纯函数；解析失败返回 null，不阻塞页面）----

/** Kimi detail_json.sources.balance：开放平台按量余额分项（单位元） */
export interface KimiBalanceDetail {
  available?: number
  voucher?: number
  cash?: number
  currency?: string
}

/** Kimi detail_json.sources.coding_usages：套餐 5h 滚动窗口 + 月度池（非官方接口，best-effort） */
export interface KimiCodingDetail {
  month_total?: number | null
  month_used?: number | null
  month_remaining?: number | null
  window_used_percent?: number | null
  window_remaining_percent?: number | null
  window_reset_at?: string | null
}

export interface KimiQuotaDetail {
  balance: KimiBalanceDetail | null
  coding: KimiCodingDetail | null
  /** coding usages 拉取失败的降级原因（余额主数据源不受影响，照常展示） */
  codingError: string | null
}

export function parseKimiQuotaDetail(
  snapshot: QuotaSnapshot | null | undefined,
): KimiQuotaDetail | null {
  const raw = snapshot?.detail_json
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as {
      sources?: {
        balance?: KimiBalanceDetail | null
        coding_usages?: KimiCodingDetail | null
        coding_usages_error?: string | null
      }
    }
    return {
      balance: parsed.sources?.balance ?? null,
      coding: parsed.sources?.coding_usages ?? null,
      codingError: parsed.sources?.coding_usages_error ?? null,
    }
  } catch {
    return null
  }
}

/** MiMo detail_json：本地日志推算的套餐消耗进度（unit 恒为 credit；progress 超量时 > 1，后端不截断） */
export interface MimoQuotaDetail {
  quotaCycle: string | null
  periodStart: string | null
  progress: number | null
  unit: string | null
}

/**
 * 解析 MiMo 手录套餐总额输入：空串 = 不配置套餐（null）；
 * 非有限正数返回 undefined 表示非法输入，调用方必须拒绝保存——
 * NaN 经 JSON 序列化会变 null，静默清掉已录总额，必须在入库前拦下。
 */
export function parseQuotaTotalInput(raw: string): number | null | undefined {
  const trimmed = raw.trim()
  if (trimmed === '') return null
  const parsed = Number(trimmed)
  if (!Number.isFinite(parsed) || parsed <= 0) return undefined
  return parsed
}

export function parseMimoQuotaDetail(
  snapshot: QuotaSnapshot | null | undefined,
): MimoQuotaDetail | null {
  const raw = snapshot?.detail_json
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as {
      quota_cycle?: string
      period_start?: string
      progress?: number
      unit?: string
    }
    return {
      quotaCycle: parsed.quota_cycle ?? null,
      periodStart: parsed.period_start ?? null,
      progress: parsed.progress ?? null,
      unit: parsed.unit ?? null,
    }
  } catch {
    return null
  }
}

/** DeepSeek detail_json：按量余额的币种分项 */
export interface DeepseekBalanceItem {
  currency?: string
  total?: number
  granted?: number
  topped_up?: number
}

export interface DeepseekQuotaDetail {
  isAvailable: boolean | null
  balances: DeepseekBalanceItem[]
}

export function parseDeepseekQuotaDetail(
  snapshot: QuotaSnapshot | null | undefined,
): DeepseekQuotaDetail | null {
  const raw = snapshot?.detail_json
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as {
      is_available?: boolean
      balances?: DeepseekBalanceItem[]
    }
    return {
      isAvailable: parsed.is_available ?? null,
      balances: parsed.balances ?? [],
    }
  } catch {
    return null
  }
}

/** M1 多平台用量 store：聚合数据按当前选中平台过滤，配额/账户按平台组织 */
export const useUsageStore = defineStore('usage', () => {
  /** 平台 Tab 当前选中项（all = 总览/跨平台合计） */
  const activeFilter = ref<PlatformFilter>('all')
  /** 总览卡片墙：各平台 + 合计的今日摘要 */
  const todayByFilter = ref<Partial<Record<PlatformFilter, UsageSummary>>>({})
  /** 当前选中过滤的各范围汇总（Day / Week / Month / Last5Hours），读取不阻塞 */
  const summaries = ref<Partial<Record<UsageRange, UsageSummary>>>({})
  const trendRange = ref<TrendRange>('Day')
  const series = ref<UsageSeries | null>(null)
  const byProject = ref<UsageGroup[]>([])
  const byModel = ref<UsageGroup[]>([])

  /** 配额快照按平台缓存（quota_latest 只读本地，quota_refresh 才走网络/推算） */
  const quotas = ref<Partial<Record<Platform, QuotaSnapshot | null>>>({})
  /** 配额刷新失败原因（按平台）；unauthorized 时 UI 引导去设置页 */
  const quotaErrors = ref<Partial<Record<Platform, CommandError | null>>>({})
  const quotaLoading = ref<Partial<Record<Platform, boolean>>>({})

  /** 各平台账户配置（凭据本体在 keyring，这里只有引用与标记） */
  const accounts = ref<Partial<Record<Platform, PlatformAccountInfo | null>>>({})

  const scanning = ref(false)
  const scanError = ref<string | null>(null)
  const lastScanAt = ref<string | null>(null)
  /** 最近一次全平台扫描的分平台报告（单平台失败不影响其他平台） */
  const lastScanReports = ref<PlatformScanReport[]>([])
  const initialized = ref(false)
  /** 初始化/聚合加载失败原因；非空时页面显示错误横幅，可重试 */
  const loadError = ref<CommandError | null>(null)
  /**
   * 聚合数据（series / byProject / byModel）写请求序号：loadAggregates 与 setTrendRange 共用。
   * 每个发起点 bump、落地前校验，快速切 Tab / 切范围时慢的旧响应直接丢弃，不覆盖新数据。
   */
  let aggregatesRequestId = 0

  const todaySummary = computed(() => summaries.value.Day ?? null)
  const last5Summary = computed(() => summaries.value.Last5Hours ?? null)
  const monthSummary = computed(() => summaries.value.Month ?? null)

  const kimiQuotaDetail = computed(() => parseKimiQuotaDetail(quotas.value.kimi))
  const mimoQuotaDetail = computed(() => parseMimoQuotaDetail(quotas.value.mimo))
  const deepseekQuotaDetail = computed(() => parseDeepseekQuotaDetail(quotas.value.deepseek))

  /** 本次扫描新增事件合计 / 失败平台列表（页面状态行展示用） */
  const lastScanEventsAdded = computed(() =>
    lastScanReports.value.reduce((sum, r) => sum + (r.report?.events_added ?? 0), 0),
  )
  const lastScanFailures = computed(() => lastScanReports.value.filter((r) => r.error !== null))

  /** 总览卡片墙：拉取 all + 各平台的今日摘要；失败写入 loadError（同源 DB，失败一般是系统性的） */
  async function loadOverview() {
    const filters: PlatformFilter[] = ['all', ...USAGE_PLATFORMS]
    try {
      const results = await Promise.all(filters.map((f) => api.usageSummary(f, 'Day')))
      const next: Partial<Record<PlatformFilter, UsageSummary>> = {}
      filters.forEach((f, i) => {
        next[f] = results[i]
      })
      todayByFilter.value = next
    } catch (err) {
      loadError.value = toCommandError(err)
    }
  }

  /** 拉取当前选中过滤的聚合数据（汇总 + 当前趋势范围的分组与序列），失败写入 loadError；过期响应丢弃 */
  async function loadAggregates() {
    const filter = activeFilter.value
    const range = trendRange.value
    const requestId = ++aggregatesRequestId
    try {
      const [day, last5, month, trend, projects, models] = await Promise.all([
        api.usageSummary(filter, 'Day'),
        api.usageSummary(filter, 'Last5Hours'),
        api.usageSummary(filter, 'Month'),
        api.usageSeries(filter, range),
        api.usageByProject(filter, range),
        api.usageByModel(filter, range),
      ])
      if (requestId !== aggregatesRequestId) return
      summaries.value = { ...summaries.value, Day: day, Last5Hours: last5, Month: month }
      series.value = trend
      byProject.value = projects
      byModel.value = models
    } catch (err) {
      if (requestId === aggregatesRequestId) loadError.value = toCommandError(err)
    }
  }

  /** 切换平台 Tab：清空旧过滤的展示数据后按新过滤重取 */
  async function setActiveFilter(filter: PlatformFilter) {
    if (filter === activeFilter.value) return
    activeFilter.value = filter
    summaries.value = {}
    series.value = null
    byProject.value = []
    byModel.value = []
    await loadAggregates()
  }

  /** 切换趋势范围（天/周/月），只重取与该范围相关的数据；过期响应直接丢弃 */
  async function setTrendRange(range: TrendRange) {
    trendRange.value = range
    const filter = activeFilter.value
    const requestId = ++aggregatesRequestId
    try {
      const [trend, projects, models] = await Promise.all([
        api.usageSeries(filter, range),
        api.usageByProject(filter, range),
        api.usageByModel(filter, range),
      ])
      if (requestId !== aggregatesRequestId) return
      series.value = trend
      byProject.value = projects
      byModel.value = models
    } catch (err) {
      if (requestId === aggregatesRequestId) loadError.value = toCommandError(err)
    }
  }

  /** 全平台增量扫描（sessionsDir 仅覆盖 kimi 日志目录），完成后刷新聚合与总览 */
  async function scan(sessionsDir?: string) {
    if (scanning.value) return
    scanning.value = true
    scanError.value = null
    try {
      const reports = await api.scanUsageLogs(undefined, sessionsDir)
      lastScanReports.value = reports
      lastScanAt.value = new Date().toISOString()
      // 单平台失败是常态（如日志目录不存在），只有全部失败才当作扫描错误提示
      if (reports.length > 0 && reports.every((r) => r.error !== null)) {
        scanError.value = reports
          .map((r) => `${PLATFORM_LABELS[r.platform]}：${r.error}`)
          .join('；')
      }
      await Promise.all([loadOverview(), loadAggregates()])
      // MiMo 配额是纯本地日志推算：扫描后顺手重算，进度条保持新鲜（未配置套餐则跳过，不报错）
      if (accounts.value.mimo?.quota_total != null) {
        await refreshQuota('mimo')
      }
    } catch (err) {
      scanError.value = toCommandError(err).message
    } finally {
      scanning.value = false
    }
  }

  /** 读取各平台最近一次配额快照（不触发网络），单平台失败只记到该平台的 quotaErrors */
  async function loadLatestQuotas() {
    await Promise.all(
      QUOTA_PLATFORMS.map(async (platform) => {
        try {
          quotas.value[platform] = await api.quotaLatest(platform)
        } catch (err) {
          quotaErrors.value[platform] = toCommandError(err)
        }
      }),
    )
  }

  /** 手动刷新某平台配额（Kimi/DeepSeek 走网络，MiMo 本地推算），失败进该平台的 quotaErrors */
  async function refreshQuota(platform: Platform) {
    quotaLoading.value[platform] = true
    try {
      quotas.value[platform] = await api.quotaRefresh(platform)
      quotaErrors.value[platform] = null
    } catch (err) {
      quotaErrors.value[platform] = toCommandError(err)
    } finally {
      quotaLoading.value[platform] = false
    }
  }

  /** 读取全部平台账户配置，失败写入 loadError */
  async function loadAccounts() {
    try {
      const results = await Promise.all(USAGE_PLATFORMS.map((p) => api.platformAccountGet(p)))
      const next: Partial<Record<Platform, PlatformAccountInfo | null>> = {}
      USAGE_PLATFORMS.forEach((p, i) => {
        next[p] = results[i]
      })
      accounts.value = next
    } catch (err) {
      loadError.value = toCommandError(err)
    }
  }

  /** 保存平台账户配置；apiKey 留空表示沿用 keyring 中已有 key */
  async function saveAccount(platform: Platform, input: PlatformAccountInput, apiKey?: string) {
    await api.platformAccountSet(platform, input, apiKey)
    accounts.value[platform] = await api.platformAccountGet(platform)
  }

  /**
   * 页面打开入口：先用 SQLite 缓存数据渲染（快照/聚合均为本地读），
   * 再后台触发增量扫描，扫完自动刷新聚合（不阻塞界面）。
   * 任一加载失败时错误进 loadError，initialized 回滚为 false，允许页面重试。
   */
  async function init(sessionsDir?: string) {
    if (initialized.value) return
    loadError.value = null
    await Promise.all([loadAccounts(), loadLatestQuotas(), loadOverview(), loadAggregates()])
    if (loadError.value) return
    initialized.value = true
    void scan(sessionsDir)
  }

  /** 错误横幅的重试入口：先清空错误，未初始化走 init，已初始化只重取聚合与总览 */
  async function retry(sessionsDir?: string) {
    loadError.value = null
    if (initialized.value) await Promise.all([loadOverview(), loadAggregates()])
    else await init(sessionsDir)
  }

  return {
    activeFilter,
    todayByFilter,
    summaries,
    trendRange,
    series,
    byProject,
    byModel,
    quotas,
    quotaErrors,
    quotaLoading,
    accounts,
    scanning,
    scanError,
    lastScanAt,
    lastScanReports,
    lastScanEventsAdded,
    lastScanFailures,
    initialized,
    loadError,
    todaySummary,
    last5Summary,
    monthSummary,
    kimiQuotaDetail,
    mimoQuotaDetail,
    deepseekQuotaDetail,
    loadOverview,
    loadAggregates,
    setActiveFilter,
    setTrendRange,
    scan,
    loadLatestQuotas,
    refreshQuota,
    loadAccounts,
    saveAccount,
    init,
    retry,
  }
})
