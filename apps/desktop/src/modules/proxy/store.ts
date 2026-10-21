import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import { toCommandError, type CommandError } from '@/shared/errors'
import * as api from '@/modules/proxy/api'
import type { NodeDelayResult, ProxyStatus, RuleMode } from '@toogl/core-types'

/** 状态轮询默认间隔（毫秒）；mihomo 侧是事实源，页面可见期间持续同步 */
export const PROXY_POLL_INTERVAL = 4000

export const RULE_MODE_LABELS: Record<RuleMode, string> = {
  rule: '规则',
  global: '全局',
  direct: '直连',
}

export const GROUP_TYPE_LABELS: Record<string, string> = {
  selector: '手动选择',
  urltest: '自动测速',
  fallback: '故障转移',
  loadbalance: '负载均衡',
  relay: '链式代理',
  unknown: '未知',
}

/**
 * M5 魔法工具 store：轮询 proxy_status 保持状态同步（mihomo 侧是事实源）。
 * 所有动作乐观更新 + 失败回滚，完成后立即刷新一次状态对齐事实源。
 */
export const useProxyStore = defineStore('proxy', () => {
  /** 最近一次聚合状态；null = 尚未拉到（首次加载中） */
  const status = ref<ProxyStatus | null>(null)
  /** 轮询/刷新本身失败（invoke 层错误）；proxy_status 正常返回时字段级降级不算错误 */
  const loadError = ref<CommandError | null>(null)
  /** 最近一次动作（切换/测速/开关）失败原因，页面横幅展示 */
  const actionError = ref<CommandError | null>(null)

  /** 测速中分组跟踪：key 为组名，'*' 表示全部测速 */
  const testingGroups = ref<Record<string, boolean>>({})
  /** 节点切换中（key = 组名）：同组同时只允许一个切换，避免并发切换互相回滚 */
  const selecting = ref<Record<string, boolean>>({})
  const switchingMode = ref(false)
  const togglingSystemProxy = ref(false)
  const recovering = ref(false)
  /** 残留提示「忽略」：仅本次会话不再提示（后端自检标记仍在，下次启动仍会提示） */
  const leftoverDismissed = ref(false)

  let timer: ReturnType<typeof setInterval> | null = null
  /** refresh 在飞标记：上一次未完成时跳过本轮，避免慢请求（>4s）逐轮叠加 */
  let refreshing = false

  const groups = computed(() => status.value?.proxies?.groups ?? [])
  /** sidecar 可用 = 存活探测通过且启动无错误；不可用时页面整体降级 */
  const sidecarOk = computed(
    () =>
      status.value !== null && status.value.sidecar_alive && status.value.startup_error === null,
  )
  /** 残留提示可见 = 自检检出残留且本次未忽略 */
  const leftoverVisible = computed(
    () => (status.value?.system_proxy_leftover ?? false) && !leftoverDismissed.value,
  )
  const testingAll = computed(() => testingGroups.value['*'] === true)

  /** 拉取一次聚合状态；失败只记 loadError，不清空已有状态（保持旧数据可读）。在飞则跳过本轮 */
  async function refresh() {
    if (refreshing) return
    refreshing = true
    try {
      status.value = await api.proxyStatus()
      loadError.value = null
    } catch (err) {
      loadError.value = toCommandError(err)
    } finally {
      refreshing = false
    }
  }

  /** 开始轮询：立即刷新一次，之后按间隔刷新；页面不可见（document.hidden）时跳过当次 */
  function startPolling(intervalMs: number = PROXY_POLL_INTERVAL) {
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

  /** 切换节点：乐观更新组内 now，失败回滚并记 actionError，成功后立即刷新对齐；同组防重入 */
  async function selectNode(group: string, node: string) {
    if (selecting.value[group]) return
    const g = status.value?.proxies?.groups.find((gr) => gr.name === group)
    const prev = g?.now
    if (!g || prev === node) return
    selecting.value[group] = true
    actionError.value = null
    g.now = node
    try {
      await api.selectProxy(group, node)
      await refresh()
    } catch (err) {
      // 回滚时重读 status（轮询可能已整体替换对象，闭包里的旧 g 已不在树上）；
      // 仅当页面上仍显示乐观值时才回滚，避免覆盖轮询已对齐的事实源
      const current = status.value?.proxies?.groups.find((gr) => gr.name === group)
      if (current && current.now === node) current.now = prev ?? node
      actionError.value = toCommandError(err)
    } finally {
      delete selecting.value[group]
    }
  }

  /**
   * 延迟测试：group 省略 = 全部组。结果按节点名合并进现有摘要（即时反馈），
   * 随后刷新一次对齐事实源；单节点失败只体现在其自身 error/状态，不影响其他节点。
   */
  async function testDelay(group?: string) {
    const key = group ?? '*'
    if (testingGroups.value[key]) return
    testingGroups.value[key] = true
    actionError.value = null
    try {
      const results = await api.testDelay(group ?? null)
      // 先刷新对齐事实源，再把测速结果合并覆盖（/proxies 的 history 可能滞后于测速响应）
      await refresh()
      mergeDelayResults(results)
    } catch (err) {
      actionError.value = toCommandError(err)
    } finally {
      delete testingGroups.value[key]
    }
  }

  /** 把测速结果合并进 status.proxies 里对应节点（按名字匹配，找不到的忽略） */
  function mergeDelayResults(results: NodeDelayResult[]) {
    const byName = new Map(results.map((r) => [r.name, r]))
    for (const g of status.value?.proxies?.groups ?? []) {
      for (const n of g.nodes) {
        const r = byName.get(n.name)
        if (r) {
          n.delay_ms = r.delay_ms
          n.delay_status = r.delay_status
        }
      }
    }
  }

  /** 系统代理开关：乐观切换，失败回滚并记 actionError */
  async function setSystemProxy(enabled: boolean) {
    if (togglingSystemProxy.value) return
    togglingSystemProxy.value = true
    actionError.value = null
    const prev = status.value?.system_proxy?.enabled
    if (status.value?.system_proxy) status.value.system_proxy.enabled = enabled
    try {
      await api.setSystemProxy(enabled)
      await refresh()
    } catch (err) {
      if (status.value?.system_proxy && prev !== undefined) {
        status.value.system_proxy.enabled = prev
      }
      actionError.value = toCommandError(err)
    } finally {
      togglingSystemProxy.value = false
    }
  }

  /** 规则模式切换：乐观更新，失败回滚并记 actionError */
  async function switchMode(mode: RuleMode) {
    if (switchingMode.value || status.value?.rule_mode === mode) return
    switchingMode.value = true
    actionError.value = null
    const prev = status.value?.rule_mode ?? null
    if (status.value) status.value.rule_mode = mode
    try {
      await api.switchRuleMode(mode)
      await refresh()
    } catch (err) {
      if (status.value) status.value.rule_mode = prev
      actionError.value = toCommandError(err)
    } finally {
      switchingMode.value = false
    }
  }

  /** 残留恢复：还原到开启前保存的旧值，成功后刷新（自检标记随之清除） */
  async function recoverLeftover() {
    if (recovering.value) return
    recovering.value = true
    actionError.value = null
    try {
      await api.recoverSystemProxy()
      await refresh()
    } catch (err) {
      actionError.value = toCommandError(err)
    } finally {
      recovering.value = false
    }
  }

  /** 「忽略」残留提示：仅本次会话生效 */
  function dismissLeftover() {
    leftoverDismissed.value = true
  }

  return {
    status,
    loadError,
    actionError,
    testingGroups,
    testingAll,
    selecting,
    switchingMode,
    togglingSystemProxy,
    recovering,
    groups,
    sidecarOk,
    leftoverVisible,
    refresh,
    startPolling,
    stopPolling,
    selectNode,
    testDelay,
    setSystemProxy,
    switchMode,
    recoverLeftover,
    dismissLeftover,
  }
})
