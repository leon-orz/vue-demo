import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { NodeDelayResult, ProxyStatus } from '@toogl/core-types'

// 与后端交互的 api 层整体 mock，只验证 store 的轮询/动作逻辑
const mocks = vi.hoisted(() => ({
  proxyStatus: vi.fn(),
  selectProxy: vi.fn(),
  testDelay: vi.fn(),
  setSystemProxy: vi.fn(),
  switchRuleMode: vi.fn(),
  recoverSystemProxy: vi.fn(),
}))
vi.mock('@/modules/proxy/api', () => mocks)

import { useProxyStore } from './store'

/** 构造一份常见的聚合状态：sidecar 存活、一个 selector 组两个节点、规则模式、系统代理关闭 */
function statusOf(overrides: Partial<ProxyStatus> = {}): ProxyStatus {
  return {
    sidecar_alive: true,
    controller_port: 9090,
    mixed_port: 7890,
    startup_error: null,
    proxies: {
      groups: [
        {
          name: 'Proxy',
          group_type: 'selector',
          now: '节点A',
          nodes: [
            { name: '节点A', node_type: 'vless', delay_ms: 120, delay_status: 'delayed' },
            { name: '节点B', node_type: 'hysteria2', delay_ms: null, delay_status: 'untested' },
          ],
        },
        {
          name: 'Auto',
          group_type: 'urltest',
          now: '节点A',
          nodes: [{ name: '节点A', node_type: 'vless', delay_ms: 120, delay_status: 'delayed' }],
        },
      ],
    },
    rule_mode: 'rule',
    system_proxy: { enabled: false, server: null },
    system_proxy_leftover: false,
    leftover_previous: null,
    remembered_selection: null,
    version_mismatch: false,
    ...overrides,
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  mocks.proxyStatus.mockImplementation(() => Promise.resolve(statusOf()))
  mocks.selectProxy.mockResolvedValue(null)
  mocks.testDelay.mockResolvedValue([])
  mocks.setSystemProxy.mockResolvedValue(null)
  mocks.switchRuleMode.mockResolvedValue(null)
  mocks.recoverSystemProxy.mockResolvedValue(null)
})

afterEach(() => {
  vi.useRealTimers()
})

describe('初始加载与轮询', () => {
  it('refresh 拉取聚合状态并清空 loadError', async () => {
    const store = useProxyStore()
    await store.refresh()
    expect(store.status?.sidecar_alive).toBe(true)
    expect(store.groups).toHaveLength(2)
    expect(store.loadError).toBeNull()
    expect(store.sidecarOk).toBe(true)
  })

  it('refresh 失败时错误进 loadError，不清空已有状态', async () => {
    const store = useProxyStore()
    await store.refresh()
    mocks.proxyStatus.mockRejectedValueOnce({ kind: 'network', message: 'invoke 失败' })
    await store.refresh()
    expect(store.loadError).toEqual({ kind: 'network', message: 'invoke 失败' })
    expect(store.status?.sidecar_alive).toBe(true)
  })

  it('startPolling 立即刷新一次并按间隔轮询，stopPolling 后停止', async () => {
    vi.useFakeTimers()
    const store = useProxyStore()
    store.startPolling(1000)
    await vi.advanceTimersByTimeAsync(0)
    expect(mocks.proxyStatus).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(3000)
    expect(mocks.proxyStatus).toHaveBeenCalledTimes(4)

    store.stopPolling()
    await vi.advanceTimersByTimeAsync(3000)
    expect(mocks.proxyStatus).toHaveBeenCalledTimes(4)
  })

  it('页面不可见（document.hidden）时跳过当次轮询', async () => {
    vi.useFakeTimers()
    const hiddenSpy = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true)
    const store = useProxyStore()
    store.startPolling(1000)
    await vi.advanceTimersByTimeAsync(0)
    expect(mocks.proxyStatus).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(2000)
    expect(mocks.proxyStatus).toHaveBeenCalledTimes(1)

    hiddenSpy.mockReturnValue(false)
    await vi.advanceTimersByTimeAsync(1000)
    expect(mocks.proxyStatus).toHaveBeenCalledTimes(2)
    store.stopPolling()
    hiddenSpy.mockRestore()
  })

  it('重复 startPolling 不会叠加定时器', async () => {
    vi.useFakeTimers()
    const store = useProxyStore()
    store.startPolling(1000)
    store.startPolling(1000)
    await vi.advanceTimersByTimeAsync(1000)
    expect(mocks.proxyStatus).toHaveBeenCalledTimes(2)
    store.stopPolling()
  })
})

describe('节点切换', () => {
  it('切换成功：乐观更新 now，命令带组/节点参数，随后刷新对齐', async () => {
    const store = useProxyStore()
    await store.refresh()
    // 切换成功后后端事实源更新，刷新拿到新选中节点
    const after = statusOf()
    after.proxies!.groups[0].now = '节点B'
    mocks.proxyStatus.mockResolvedValue(after)

    await store.selectNode('Proxy', '节点B')
    expect(mocks.selectProxy).toHaveBeenCalledWith('Proxy', '节点B')
    expect(store.groups[0].now).toBe('节点B')
    // 动作后即时刷新一次
    expect(mocks.proxyStatus).toHaveBeenCalledTimes(2)
    expect(store.actionError).toBeNull()
  })

  it('切换失败：now 回滚到原节点，错误进 actionError', async () => {
    const store = useProxyStore()
    await store.refresh()
    mocks.selectProxy.mockRejectedValue({ kind: 'network', message: 'mihomo 不可达' })

    await store.selectNode('Proxy', '节点B')
    expect(store.groups[0].now).toBe('节点A')
    expect(store.actionError).toEqual({ kind: 'network', message: 'mihomo 不可达' })
  })

  it('切换当前已选中的节点时不发请求', async () => {
    const store = useProxyStore()
    await store.refresh()
    await store.selectNode('Proxy', '节点A')
    expect(mocks.selectProxy).not.toHaveBeenCalled()
  })
})

describe('延迟测试', () => {
  it('组级测速：结果按节点名合并进摘要，单节点失败不影响其他节点', async () => {
    const store = useProxyStore()
    await store.refresh()
    const results: NodeDelayResult[] = [
      { name: '节点A', delay_ms: 88, delay_status: 'delayed', error: null },
      { name: '节点B', delay_ms: null, delay_status: 'timeout', error: '超时' },
    ]
    mocks.testDelay.mockResolvedValue(results)

    await store.testDelay('Proxy')
    expect(mocks.testDelay).toHaveBeenCalledWith('Proxy')
    expect(store.groups[0].nodes[0].delay_ms).toBe(88)
    expect(store.groups[0].nodes[1].delay_status).toBe('timeout')
    expect(store.testingGroups.Proxy ?? false).toBe(false)
    // 合并后又刷新一次对齐事实源
    expect(mocks.proxyStatus).toHaveBeenCalledTimes(2)
  })

  it('全部测速：group 传 null，testingAll 期间为 true', async () => {
    const store = useProxyStore()
    await store.refresh()
    let resolveTest!: (v: NodeDelayResult[]) => void
    mocks.testDelay.mockReturnValue(
      new Promise((resolve) => {
        resolveTest = resolve
      }),
    )

    const pending = store.testDelay()
    expect(mocks.testDelay).toHaveBeenCalledWith(null)
    expect(store.testingAll).toBe(true)
    resolveTest([])
    await pending
    expect(store.testingAll).toBe(false)
  })

  it('测速命令失败：错误进 actionError，不清空已有延迟数据', async () => {
    const store = useProxyStore()
    await store.refresh()
    mocks.testDelay.mockRejectedValue({ kind: 'network', message: 'mihomo 不可达' })

    await store.testDelay('Proxy')
    expect(store.actionError?.kind).toBe('network')
    expect(store.groups[0].nodes[0].delay_ms).toBe(120)
  })
})

describe('系统代理开关', () => {
  it('开启成功：乐观置位并刷新', async () => {
    const store = useProxyStore()
    await store.refresh()
    mocks.proxyStatus.mockResolvedValue(
      statusOf({ system_proxy: { enabled: true, server: '127.0.0.1:7890' } }),
    )

    await store.setSystemProxy(true)
    expect(mocks.setSystemProxy).toHaveBeenCalledWith(true)
    expect(store.status?.system_proxy?.enabled).toBe(true)
    expect(store.actionError).toBeNull()
  })

  it('开启失败：开关回滚到关闭，错误进 actionError', async () => {
    const store = useProxyStore()
    await store.refresh()
    mocks.setSystemProxy.mockRejectedValue({ kind: 'internal', message: '注册表写入失败' })

    await store.setSystemProxy(true)
    expect(store.status?.system_proxy?.enabled).toBe(false)
    expect(store.actionError).toEqual({ kind: 'internal', message: '注册表写入失败' })
  })
})

describe('规则模式切换', () => {
  it('切换成功更新模式；失败回滚', async () => {
    const store = useProxyStore()
    await store.refresh()

    mocks.proxyStatus.mockResolvedValue(statusOf({ rule_mode: 'global' }))
    await store.switchMode('global')
    expect(mocks.switchRuleMode).toHaveBeenCalledWith('global')
    expect(store.status?.rule_mode).toBe('global')

    mocks.switchRuleMode.mockRejectedValue({ kind: 'network', message: 'mihomo 不可达' })
    await store.switchMode('direct')
    expect(store.status?.rule_mode).toBe('global')
    expect(store.actionError?.message).toBe('mihomo 不可达')
  })
})

describe('残留提示流', () => {
  const leftoverStatus = () =>
    statusOf({
      system_proxy: { enabled: true, server: '127.0.0.1:7890' },
      system_proxy_leftover: true,
      leftover_previous: { enabled: false, server: null },
    })

  it('检出残留时 leftoverVisible，忽略后本次会话不再提示', async () => {
    mocks.proxyStatus.mockResolvedValue(leftoverStatus())
    const store = useProxyStore()
    await store.refresh()
    expect(store.leftoverVisible).toBe(true)

    store.dismissLeftover()
    expect(store.leftoverVisible).toBe(false)
    // 后端标记仍在，只是本次忽略
    expect(store.status?.system_proxy_leftover).toBe(true)
  })

  it('恢复成功：调用 recover_system_proxy 并刷新，自检标记清除后提示消失', async () => {
    mocks.proxyStatus.mockResolvedValueOnce(leftoverStatus())
    const store = useProxyStore()
    await store.refresh()
    expect(store.leftoverVisible).toBe(true)

    await store.recoverLeftover()
    expect(mocks.recoverSystemProxy).toHaveBeenCalledTimes(1)
    // 刷新后拿到的是无残留状态
    expect(store.status?.system_proxy_leftover).toBe(false)
    expect(store.leftoverVisible).toBe(false)
  })

  it('恢复失败：错误进 actionError，提示保留', async () => {
    mocks.proxyStatus.mockResolvedValue(leftoverStatus())
    const store = useProxyStore()
    await store.refresh()
    mocks.recoverSystemProxy.mockRejectedValue({ kind: 'internal', message: '注册表写入失败' })

    await store.recoverLeftover()
    expect(store.actionError?.message).toBe('注册表写入失败')
    expect(store.leftoverVisible).toBe(true)
    expect(store.recovering).toBe(false)
  })
})

describe('降级态', () => {
  it('startup_error 非空或 sidecar 未存活时 sidecarOk 为 false', async () => {
    mocks.proxyStatus.mockResolvedValue(
      statusOf({ sidecar_alive: false, startup_error: 'mihomo 二进制缺失', proxies: null }),
    )
    const store = useProxyStore()
    await store.refresh()
    expect(store.sidecarOk).toBe(false)
    expect(store.groups).toHaveLength(0)
  })
})
