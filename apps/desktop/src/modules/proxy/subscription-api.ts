import { invoke } from '@tauri-apps/api/core'
import type { ImportResult, SubscriptionInfo } from '@toogl/core-types'

/** 与 src-tauri/src/commands/subscription.rs 一一对应；参数/返回类型一律来自 ts-rs 生成物 */

/** 订阅/手动导入的聚合状态（各字段独立降级，未配置订阅不算错误） */
export function subscriptionStatus(): Promise<SubscriptionInfo> {
  return invoke('subscription_status')
}

/** 保存订阅链接并立即拉取一次；返回最新聚合状态 */
export function subscriptionSetUrl(url: string): Promise<SubscriptionInfo> {
  return invoke('subscription_set_url', { url })
}

/** 手动刷新订阅（拉取失败时保留旧节点，原因见返回的 last_error） */
export function subscriptionUpdate(): Promise<SubscriptionInfo> {
  return invoke('subscription_update')
}

/** 手动粘贴导入分享链接文本（坏行不拖垮整体，失败明细在 ImportResult.failures） */
export function importShareText(text: string): Promise<ImportResult> {
  return invoke('import_share_text', { text })
}
