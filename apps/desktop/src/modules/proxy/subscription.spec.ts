import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { ImportResult, SubscriptionInfo } from '@toogl/core-types'

// 与后端交互的 api 层整体 mock，只验证 store 的状态/动作逻辑
const mocks = vi.hoisted(() => ({
  subscriptionStatus: vi.fn(),
  subscriptionSetUrl: vi.fn(),
  subscriptionUpdate: vi.fn(),
  importShareText: vi.fn(),
}))
vi.mock('@/modules/proxy/subscription-api', () => mocks)

import { useSubscriptionStore } from './subscription'

function statusOf(patch: Partial<SubscriptionInfo> = {}): SubscriptionInfo {
  return {
    url: 'https://example.com/sub',
    last_updated: '2026-01-01T00:00:00Z',
    node_count: 12,
    manual_count: 3,
    updating: false,
    last_error: null,
    ...patch,
  }
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  mocks.subscriptionStatus.mockResolvedValue(statusOf())
  mocks.subscriptionSetUrl.mockResolvedValue(statusOf())
  mocks.subscriptionUpdate.mockResolvedValue(statusOf())
  mocks.importShareText.mockResolvedValue({ imported: 2, failures: [] } satisfies ImportResult)
})

describe('订阅状态刷新', () => {
  it('refresh 成功填充 status 并清空 loadError', async () => {
    const store = useSubscriptionStore()
    await store.refresh()
    expect(store.status?.node_count).toBe(12)
    expect(store.loadError).toBeNull()
    expect(mocks.subscriptionStatus).toHaveBeenCalledTimes(1)
  })

  it('refresh 失败记 loadError，不清空旧状态', async () => {
    const store = useSubscriptionStore()
    await store.refresh()
    const prev = store.status

    mocks.subscriptionStatus.mockRejectedValueOnce({ kind: 'network', message: '连接超时' })
    await store.refresh()
    expect(store.loadError).toEqual({ kind: 'network', message: '连接超时' })
    expect(store.status).toBe(prev)
    expect(store.status?.node_count).toBe(12)
  })

  it('updating 由 status.updating 派生', async () => {
    mocks.subscriptionStatus.mockResolvedValue(statusOf({ updating: true }))
    const store = useSubscriptionStore()
    await store.refresh()
    expect(store.updating).toBe(true)
  })
})

describe('订阅动作', () => {
  it('saveUrl 成功后触发 refresh 对齐状态', async () => {
    const store = useSubscriptionStore()
    await store.saveUrl('https://example.com/new')
    expect(mocks.subscriptionSetUrl).toHaveBeenCalledWith('https://example.com/new')
    expect(mocks.subscriptionStatus).toHaveBeenCalledTimes(1)
    expect(store.actionError).toBeNull()
  })

  it('updateNow 成功后触发 refresh 对齐状态', async () => {
    const store = useSubscriptionStore()
    await store.updateNow()
    expect(mocks.subscriptionUpdate).toHaveBeenCalledTimes(1)
    expect(mocks.subscriptionStatus).toHaveBeenCalledTimes(1)
    expect(store.actionError).toBeNull()
  })

  it('动作失败记 actionError，不触发 refresh', async () => {
    mocks.subscriptionSetUrl.mockRejectedValueOnce({ kind: 'internal', message: '订阅链接无效' })
    const store = useSubscriptionStore()
    await store.saveUrl('bad')
    expect(store.actionError).toEqual({ kind: 'internal', message: '订阅链接无效' })
    expect(mocks.subscriptionStatus).not.toHaveBeenCalled()
  })

  it('importText 成功存 lastImportResult 并触发 refresh', async () => {
    const result: ImportResult = {
      imported: 5,
      failures: [{ line: 'ss://broken', reason: '链接格式不支持' }],
    }
    mocks.importShareText.mockResolvedValue(result)
    const store = useSubscriptionStore()
    await store.importText('ss://a\nss://broken')
    expect(mocks.importShareText).toHaveBeenCalledWith('ss://a\nss://broken')
    expect(store.lastImportResult).toEqual(result)
    expect(store.lastImportResult?.failures).toHaveLength(1)
    expect(mocks.subscriptionStatus).toHaveBeenCalledTimes(1)
    expect(store.importing).toBe(false)
  })

  it('importText 失败记 actionError，lastImportResult 保持旧值', async () => {
    const store = useSubscriptionStore()
    mocks.importShareText.mockRejectedValueOnce({ kind: 'internal', message: '导入失败' })
    await store.importText('garbage')
    expect(store.actionError).toEqual({ kind: 'internal', message: '导入失败' })
    expect(store.lastImportResult).toBeNull()
    expect(store.importing).toBe(false)
  })

  it('防重入：importing 中重复调用不再发请求', async () => {
    let resolveImport!: (v: ImportResult) => void
    mocks.importShareText.mockReturnValue(
      new Promise((resolve) => {
        resolveImport = resolve
      }),
    )
    const store = useSubscriptionStore()
    const first = store.importText('ss://a')
    // 第一次还在进行中，第二次应被 guard 拦截
    const second = store.importText('ss://b')
    expect(store.importing).toBe(true)
    resolveImport({ imported: 1, failures: [] })
    await Promise.all([first, second])
    expect(mocks.importShareText).toHaveBeenCalledTimes(1)
    expect(mocks.importShareText).toHaveBeenCalledWith('ss://a')
    expect(store.importing).toBe(false)
  })
})
