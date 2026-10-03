import { invoke } from '@tauri-apps/api/core'
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

/** 与 src-tauri/src/commands/usage.rs 一一对应；参数/返回类型一律来自 ts-rs 生成物 */

/**
 * 手动触发增量扫描（platform 不传 = 全平台；单平台失败不影响其他平台，
 * 结果/错误见每个 PlatformScanReport 元素）。sessionsDir 仅作用于 kimi 日志目录覆盖。
 */
export function scanUsageLogs(
  platform?: Platform,
  sessionsDir?: string,
): Promise<PlatformScanReport[]> {
  return invoke('scan_usage_logs', { platform, sessionsDir })
}

/** 聚合用量总览（天/周/月/最近5小时）；platform='all' 为跨平台合计 */
export function usageSummary(platform: PlatformFilter, range: UsageRange): Promise<UsageSummary> {
  return invoke('usage_summary', { platform, range })
}

export function usageByProject(platform: PlatformFilter, range: UsageRange): Promise<UsageGroup[]> {
  return invoke('usage_by_project', { platform, range })
}

export function usageByModel(platform: PlatformFilter, range: UsageRange): Promise<UsageGroup[]> {
  return invoke('usage_by_model', { platform, range })
}

/** 用量趋势时间序列（Day 按小时、Week/Month 按日） */
export function usageSeries(platform: PlatformFilter, range: UsageRange): Promise<UsageSeries> {
  return invoke('usage_series', { platform, range })
}

/** 拉取/重算平台配额并写快照 */
export function quotaRefresh(platform: Platform): Promise<QuotaSnapshot> {
  return invoke('quota_refresh', { platform })
}

/** 最近一次配额快照（不触发网络请求） */
export function quotaLatest(platform: Platform): Promise<QuotaSnapshot | null> {
  return invoke('quota_latest', { platform })
}

export function platformAccountGet(platform: Platform): Promise<PlatformAccountInfo | null> {
  return invoke('platform_account_get', { platform })
}

/** apiKey 传空字符串/省略表示沿用 keyring 中已有 key */
export function platformAccountSet(
  platform: Platform,
  input: PlatformAccountInput,
  apiKey?: string,
): Promise<void> {
  return invoke('platform_account_set', { platform, input, apiKey })
}
