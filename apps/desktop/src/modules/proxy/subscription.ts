import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { toCommandError, type CommandError } from '@/shared/errors'
import * as api from '@/modules/proxy/subscription-api'
import type { ImportResult, SubscriptionInfo } from '@toogl/core-types'

/** 订阅状态轮询默认间隔（毫秒），与 proxy store 同节奏 */
export const SUBSCRIPTION_POLL_INTERVAL = 4000

/**
 * M5-07 节点导入 store：轮询 subscription_status 保持订阅卡片状态同步。
 * 动作统一模式：防重入 → 清 actionError → 调 api → 成功后 refresh 对齐事实源，
 * 失败记 actionError（importText 的成功结果另存 lastImportResult 供 UI 展示失败明细）。
 */
export const useSubscriptionStore = defineStore('subscription', () => {
  /** 最近一次聚合状态；null = 尚未拉到（首次加载中） */
  const status = ref<SubscriptionInfo | null>(null)
  /** 轮询/刷新本身失败（invoke 层错误）；不清空已有状态 */
  const loadError = ref<CommandError | null>(null)
  /** 最近一次动作（保存链接/手动更新/导入）失败原因，页面横幅展示 */
  const actionError = ref<CommandError | null>(null)

  /** 保存链接 / 手动更新的本地动作锁（防重入；与 status.updating 共同为 UI 加载态） */
  const updatingAction = ref(false)
  /** 粘贴导入进行中 */
  const importing = ref(false)
  /** 最近一次粘贴导入的结果（含逐条失败明细），null = 尚未导入过 */
  const lastImportResult = ref<ImportResult | null>(null)

  /** 更新中 = 本地动作锁或后端 updating 标记（UI 据此禁用按钮、显示加载态） */
  const updating = computed(() => updatingAction.value || (status.value?.updating ?? false))

  let timer: ReturnType<typeof setInterval> | null = null
  /** refresh 在飞标记：上一次未完成时跳过本轮，避免慢请求（>4s）逐轮叠加 */
  let refreshing = false

  /** 拉取一次聚合状态；失败只记 loadError，不清空已有状态（保持旧数据可读）。在飞则跳过本轮 */
  async function refresh() {
    if (refreshing) return
    refreshing = true
    try {
      status.value = await api.subscriptionStatus()
      loadError.value = null
    } catch (err) {
      loadError.value = toCommandError(err)
    } finally {
      refreshing = false
    }
  }

  /** 开始轮询：立即刷新一次，之后按间隔刷新；页面不可见（document.hidden）时跳过当次 */
  function startPolling(intervalMs: number = SUBSCRIPTION_POLL_INTERVAL) {
    if (timer !== null) return
    void refresh()
    timer = setInterval(() => {
      if (typeof document !== 'undefined' && document.hidden) return
      void refresh()
    }, intervalMs)
  }

  /** 停止轮询（离开页面时调用） */
  function stopPolling() {
    if (timer !== null) {
      clearInterval(timer)
      timer = null
    }
  }

  /** 保存订阅链接并立即拉取一次；成功后刷新对齐事实源 */
  async function saveUrl(url: string) {
    if (updatingAction.value) return
    updatingAction.value = true
    actionError.value = null
    try {
      await api.subscriptionSetUrl(url)
      await refresh()
    } catch (err) {
      actionError.value = toCommandError(err)
    } finally {
      updatingAction.value = false
    }
  }

  /** 手动刷新订阅；拉取失败时后端保留旧节点，原因见 status.last_error */
  async function updateNow() {
    if (updatingAction.value) return
    updatingAction.value = true
    actionError.value = null
    try {
      await api.subscriptionUpdate()
      await refresh()
    } catch (err) {
      actionError.value = toCommandError(err)
    } finally {
      updatingAction.value = false
    }
  }

  /** 手动粘贴导入分享链接文本；成功结果存 lastImportResult（含失败明细） */
  async function importText(text: string) {
    if (importing.value) return
    importing.value = true
    actionError.value = null
    try {
      lastImportResult.value = await api.importShareText(text)
      await refresh()
    } catch (err) {
      actionError.value = toCommandError(err)
    } finally {
      importing.value = false
    }
  }

  return {
    status,
    loadError,
    actionError,
    updating,
    importing,
    lastImportResult,
    refresh,
    startPolling,
    stopPolling,
    saveUrl,
    updateNow,
    importText,
  }
})
