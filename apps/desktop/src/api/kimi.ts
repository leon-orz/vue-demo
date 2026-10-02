import type {
  PlatformAccountInfo,
  QuotaSnapshot,
  ScanReport,
  UsageGroup,
  UsageRange,
  UsageSeries,
  UsageSummary,
} from '@toogl/core-types'

import * as usage from '@/api/usage'

/**
 * Kimi 平台兼容封装（S1-4 命令层泛化后的过渡层，下一波页面泛化时随 stores/kimi 一起并入 usage store）。
 * 函数名与旧 api/kimi.ts 保持一致，内部全部转调 api/usage 的泛化命令。
 */

/** 手动触发增量扫描（sessionsDir 留空走默认目录）；平台级失败以 CommandError 形态抛出 */
export async function scanKimiLogs(sessionsDir?: string): Promise<ScanReport> {
  const results = await usage.scanUsageLogs('kimi', sessionsDir)
  const kimi = results.find((r) => r.platform === 'kimi')
  if (kimi?.report) return kimi.report
  throw { kind: 'internal', message: kimi?.error ?? '扫描失败：无结果' }
}

export function kimiUsageSummary(range: UsageRange): Promise<UsageSummary> {
  return usage.usageSummary('kimi', range)
}

export function kimiUsageByProject(range: UsageRange): Promise<UsageGroup[]> {
  return usage.usageByProject('kimi', range)
}

export function kimiUsageByModel(range: UsageRange): Promise<UsageGroup[]> {
  return usage.usageByModel('kimi', range)
}

/** 用量趋势时间序列（Day 按小时、Week/Month 按日） */
export function kimiUsageSeries(range: UsageRange): Promise<UsageSeries> {
  return usage.usageSeries('kimi', range)
}

/** 最近一次配额快照（不触发网络请求）。Kimi 余额明细在 detail_json.sources.balance */
export function kimiBalanceLatest(): Promise<QuotaSnapshot | null> {
  return usage.quotaLatest('kimi')
}

export function kimiBalanceRefresh(): Promise<QuotaSnapshot> {
  return usage.quotaRefresh('kimi')
}

export function kimiAccountGet(): Promise<PlatformAccountInfo | null> {
  return usage.platformAccountGet('kimi')
}

/** apiKey 传空字符串/省略表示沿用 keyring 中已有 key */
export function kimiAccountSet(
  account: { name: string; api_base: string; enabled: boolean },
  apiKey?: string,
): Promise<void> {
  return usage.platformAccountSet(
    'kimi',
    {
      platform: 'kimi',
      name: account.name,
      api_base: account.api_base || null,
      quota_total: null,
      quota_cycle: null,
      enabled: account.enabled,
    },
    apiKey,
  )
}
