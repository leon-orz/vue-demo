import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import * as api from '@/api/kimi'
import type {
  KimiAccountInfo,
  KimiBalance,
  KimiScanReport,
  KimiUsageGroup,
  KimiUsageRange,
  KimiUsageSeries,
  KimiUsageSummary,
} from '@toogl/core-types'

/** 趋势图可选范围（Last5Hours 不参与趋势切换） */
export type TrendRange = 'Day' | 'Week' | 'Month'

export const useKimiStore = defineStore('kimi', () => {
  /** 各范围汇总（Day / Week / Month / Last5Hours），数据来自 SQLite 聚合表，读取不阻塞 */
  const summaries = ref<Partial<Record<KimiUsageRange, KimiUsageSummary>>>({})
  const trendRange = ref<TrendRange>('Day')
  const series = ref<KimiUsageSeries | null>(null)
  const byProject = ref<KimiUsageGroup[]>([])
  const byModel = ref<KimiUsageGroup[]>([])

  const balance = ref<KimiBalance | null>(null)
  const balanceError = ref<string | null>(null)
  const balanceLoading = ref(false)

  const account = ref<KimiAccountInfo | null>(null)

  const scanning = ref(false)
  const scanError = ref<string | null>(null)
  const lastScanAt = ref<string | null>(null)
  const lastScanReport = ref<KimiScanReport | null>(null)
  const initialized = ref(false)

  const todaySummary = computed(() => summaries.value.Day ?? null)
  const last5Summary = computed(() => summaries.value.Last5Hours ?? null)
  const monthSummary = computed(() => summaries.value.Month ?? null)

  /** 拉取全部聚合数据（汇总 + 当前趋势范围的分组与序列） */
  async function loadAggregates() {
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
  }

  /** 切换趋势范围（天/周/月），只重取与该范围相关的数据 */
  async function setTrendRange(range: TrendRange) {
    trendRange.value = range
    const [trend, projects, models] = await Promise.all([
      api.kimiUsageSeries(range),
      api.kimiUsageByProject(range),
      api.kimiUsageByModel(range),
    ])
    series.value = trend
    byProject.value = projects
    byModel.value = models
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
      scanError.value = String(err)
    } finally {
      scanning.value = false
    }
  }

  /** 读取最近一次余额快照（不触发网络） */
  async function loadLatestBalance() {
    balance.value = await api.kimiBalanceLatest()
  }

  /** 手动刷新余额（走网络，失败时错误文案进 balanceError） */
  async function refreshBalance() {
    balanceLoading.value = true
    try {
      balance.value = await api.kimiBalanceRefresh()
      balanceError.value = null
    } catch (err) {
      balanceError.value = String(err)
    } finally {
      balanceLoading.value = false
    }
  }

  async function loadAccount() {
    account.value = await api.kimiAccountGet()
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
   */
  async function init(sessionsDir?: string) {
    if (initialized.value) return
    initialized.value = true
    await Promise.all([loadAccount(), loadLatestBalance(), loadAggregates()])
    void scan(sessionsDir)
  }

  return {
    summaries,
    trendRange,
    series,
    byProject,
    byModel,
    balance,
    balanceError,
    balanceLoading,
    account,
    scanning,
    scanError,
    lastScanAt,
    lastScanReport,
    initialized,
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
  }
})
