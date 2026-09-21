import { invoke } from '@tauri-apps/api/core'
import type {
  KimiAccountInfo,
  KimiAccountInput,
  KimiBalance,
  KimiScanReport,
  KimiUsageGroup,
  KimiUsageRange,
  KimiUsageSeries,
  KimiUsageSummary,
} from '@toogl/core-types'

/** 与 src-tauri/src/commands/kimi.rs 一一对应；参数/返回类型一律来自 ts-rs 生成物 */

/** 手动触发增量扫描（sessionsDir 留空走默认目录） */
export function scanKimiLogs(sessionsDir?: string): Promise<KimiScanReport> {
  return invoke('scan_kimi_logs', { sessionsDir })
}

export function kimiUsageSummary(range: KimiUsageRange): Promise<KimiUsageSummary> {
  return invoke('kimi_usage_summary', { range })
}

export function kimiUsageByProject(range: KimiUsageRange): Promise<KimiUsageGroup[]> {
  return invoke('kimi_usage_by_project', { range })
}

export function kimiUsageByModel(range: KimiUsageRange): Promise<KimiUsageGroup[]> {
  return invoke('kimi_usage_by_model', { range })
}

/** 用量趋势时间序列（Day 按小时、Week/Month 按日） */
export function kimiUsageSeries(range: KimiUsageRange): Promise<KimiUsageSeries> {
  return invoke('kimi_usage_series', { range })
}

/** 最近一次余额快照（不触发网络请求） */
export function kimiBalanceLatest(): Promise<KimiBalance | null> {
  return invoke('kimi_balance_latest')
}

export function kimiBalanceRefresh(): Promise<KimiBalance> {
  return invoke('kimi_balance_refresh')
}

export function kimiAccountGet(): Promise<KimiAccountInfo | null> {
  return invoke('kimi_account_get')
}

/** apiKey 传空字符串/省略表示沿用 keyring 中已有 key */
export function kimiAccountSet(account: KimiAccountInput, apiKey?: string): Promise<void> {
  return invoke('kimi_account_set', { account, apiKey })
}
