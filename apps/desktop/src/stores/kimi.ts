import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { toCommandError, type CommandError } from '@/api/errors'
import * as api from '@/api/kimi'
import type {
  PlatformAccountInfo,
  QuotaSnapshot,
  ScanReport,
  UsageGroup,
  UsageRange,
  UsageSeries,
  UsageSummary,
} from '@toogl/core-types'

/** 趋势图可选范围（Last5Hours 不参与趋势切换） */
export type TrendRange = Exclude<UsageRange, 'Last5Hours'>

/** Kimi 配额快照 detail_json.sources.balance 的明细（开放平台余额分项，单位元） */
export interface KimiBalanceDetail {
  available?: number
  voucher?: number
  cash?: number
}

export const useKimiStore = defineStore('kimi', () => {
  /** 各范围汇总（Day / Week / Month / Last5Hours），数据来自 SQLite 聚合表，读取不阻塞 */
  const summaries = ref<Partial<Record<UsageRange, UsageSummary>>>({})
  const trendRange = ref<TrendRange>('Day')
  const series = ref<UsageSeries | null>(null)
  const byProject = ref<UsageGroup[]>([])
  const byModel = ref<UsageGroup[]>([])

  const balance = ref<QuotaSnapshot | null>(null)
  /** 余额刷新失败原因（结构化）；unauthorized 时 UI 引导去设置页 */
  const balanceError = ref<CommandError | null>(null)
  const balanceLoading = ref(false)

  const account = ref<PlatformAccountInfo | null>(null)

  const scanning = ref(false)
  const scanError = ref<string | null>(null)
  const lastScanAt = ref<string | null>(null)
  const lastScanReport = ref<ScanReport | null>(null)
  const initialized = ref(false)
  /** 初始化/聚合加载失败原因；非空时页面显示错误横幅，可重试 */
  const loadError = ref<CommandError | null>(null)
  /** 趋势切换请求序号：快速切换时丢弃过期响应，避免慢的旧请求覆盖新数据 */
  let trendRequestId = 0

  const todaySummary = computed(() => summaries.value.Day ?? null)
  const last5Summary = computed(() => summaries.value.Last5Hours ?? null)
  const monthSummary = computed(() => summaries.value.Month ?? null)

  /** 解析 balance.detail_json 里的开放平台余额分项（解析失败返回 null，不阻塞页面） */
  const balanceDetail = computed<KimiBalanceDetail | null>(() => {
    const raw = balance.value?.detail_json
    if (!raw) return null
    try {
      const parsed = JSON.parse(raw) as { sources?: { balance?: KimiBalanceDetail } }
      return parsed.sources?.balance ?? null
    } catch {
      return null
    }
  })

  /** 拉取全部聚合数据（汇总 + 当前趋势范围的分组与序列），失败写入 loadError */
  async function loadAggregates() {
    try {
      const [day, last5, month, trend, projects, models] = await Promise.all([
        api.kimiUsageSummary('Day'),
        api.kimiUsageSummary('Last5Hours'),
        api.kimiUsageSummary('Month'),
        api.kimiUsageSeries(trendRange.value),
        api.kimiUsageByProject(trendRange.value),
        api.kimiUsageByModel(trendRange.value),
      ])
      summaries.value = { ...summaries.value, Day: day, Last5Hours: last5, Month: month }
      series.value = trend
      byProject.value = projects
      byModel.value = models
    } catch (err) {
      loadError.value = toCommandError(err)
    }
  }

  /** 切换趋势范围（天/周/月），只重取与该范围相关的数据；过期响应直接丢弃 */
  async function setTrendRange(range: TrendRange) {
    trendRange.value = range
    const requestId = ++trendRequestId
    try {
      const [trend, projects, models] = await Promise.all([
        api.kimiUsageSeries(range),
        api.kimiUsageByProject(range),
        api.kimiUsageByModel(range),
      ])
      if (requestId !== trendRequestId) return
      series.value = trend
      byProject.value = projects
      byModel.value = models
    } catch (err) {
      if (requestId === trendRequestId) loadError.value = toCommandError(err)
    }
  }

  /** 增量扫描日志（sessionsDir 留空走默认目录），完成后刷新聚合数据 */
  async function scan(sessionsDir?: string) {
    if (scanning.value) return
    scanning.value = true
    scanError.value = null
    try {
      lastScanReport.value = await api.scanKimiLogs(sessionsDir || undefined)
      lastScanAt.value = new Date().toISOString()
      await loadAggregates()
    } catch (err) {
      scanError.value = toCommandError(err).message
    } finally {
      scanning.value = false
    }
  }

  /** 读取最近一次余额快照（不触发网络），失败写入 loadError */
  async function loadLatestBalance() {
    try {
      balance.value = await api.kimiBalanceLatest()
    } catch (err) {
      loadError.value = toCommandError(err)
    }
  }

  /** 手动刷新余额（走网络，失败时错误进 balanceError） */
  async function refreshBalance() {
    balanceLoading.value = true
    try {
      balance.value = await api.kimiBalanceRefresh()
      balanceError.value = null
    } catch (err) {
      balanceError.value = toCommandError(err)
    } finally {
      balanceLoading.value = false
    }
  }

  /** 读取开放平台账户配置，失败写入 loadError */
  async function loadAccount() {
    try {
      account.value = await api.kimiAccountGet()
    } catch (err) {
      loadError.value = toCommandError(err)
    }
  }

  /** 保存开放平台账户配置；apiKey 留空表示沿用 keyring 中已有 key */
  async function saveAccount(
    input: { name: string; apiBase: string; enabled: boolean },
    apiKey?: string,
  ) {
    await api.kimiAccountSet(
      { name: input.name, api_base: input.apiBase, enabled: input.enabled },
      apiKey,
    )
    await loadAccount()
  }

  /**
   * 页面打开入口：先用 SQLite 缓存数据渲染（快照/聚合均为本地读），
   * 再后台触发增量扫描，扫完自动刷新聚合（不阻塞界面）。
   * 任一加载失败时错误进 loadError，initialized 回滚为 false，允许页面重试。
   */
  async function init(sessionsDir?: string) {
    if (initialized.value) return
    loadError.value = null
    await Promise.all([loadAccount(), loadLatestBalance(), loadAggregates()])
    if (loadError.value) return
    initialized.value = true
    void scan(sessionsDir)
  }

  /** 错误横幅的重试入口：先清空错误，未初始化走 init，已初始化只重取聚合 */
  async function retry(sessionsDir?: string) {
    loadError.value = null
    if (initialized.value) await loadAggregates()
    else await init(sessionsDir)
  }

  return {
    summaries,
    trendRange,
    series,
    byProject,
    byModel,
    balance,
    balanceDetail,
    balanceError,
    balanceLoading,
    account,
    scanning,
    scanError,
    lastScanAt,
    lastScanReport,
    initialized,
    loadError,
    todaySummary,
    last5Summary,
    monthSummary,
    loadAggregates,
    setTrendRange,
    scan,
    loadLatestBalance,
    refreshBalance,
    loadAccount,
    saveAccount,
    init,
    retry,
  }
})
