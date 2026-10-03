import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { PlatformAccountInput } from '@toogl/core-types'

// mock Tauri invoke，锁定每条封装发出的命令名与参数键。
// 契约坑位：Tauri 命令参数在 Rust 侧是 snake_case（如 sessions_dir），invoke 时由
// Tauri 自动把前端 camelCase 键（sessionsDir）转换为 snake_case——所以这里必须断言
// 前端发出的是 camelCase 键，写成 snake_case 或改名都会在运行时静默传 undefined。
const invokeMock = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

import * as proxyApi from './proxy'
import * as usageApi from './usage'

beforeEach(() => {
  invokeMock.mockReset().mockResolvedValue(null)
})

/** 断言最近一次调用的命令名与参数键集合（值不参与比较） */
function expectLastCall(cmd: string, keys: string[]) {
  const [actualCmd, actualArgs] = invokeMock.mock.lastCall as [string, Record<string, unknown>?]
  expect(actualCmd).toBe(cmd)
  expect(Object.keys(actualArgs ?? {}).sort()).toEqual([...keys].sort())
}

describe('api/usage.ts invoke 契约', () => {
  it('scanUsageLogs 全参数：scan_usage_logs，sessionsDir 为 camelCase', async () => {
    await usageApi.scanUsageLogs('kimi', 'D:/logs')
    expectLastCall('scan_usage_logs', ['platform', 'sessionsDir'])
    const args = invokeMock.mock.lastCall?.[1] as Record<string, unknown>
    expect(args.platform).toBe('kimi')
    expect(args.sessionsDir).toBe('D:/logs')
    // 坑位回归：禁止直接发 snake_case 键（Tauri 期望 camelCase，由它转换）
    expect('sessions_dir' in args).toBe(false)
  })

  it('scanUsageLogs 省略参数：键仍发出（undefined），由 Rust 侧 Option 兜底', async () => {
    await usageApi.scanUsageLogs()
    expectLastCall('scan_usage_logs', ['platform', 'sessionsDir'])
  })

  it('usageSummary / usageByProject / usageByModel / usageSeries：platform + range', async () => {
    await usageApi.usageSummary('all', 'Day')
    expectLastCall('usage_summary', ['platform', 'range'])

    await usageApi.usageByProject('kimi', 'Week')
    expectLastCall('usage_by_project', ['platform', 'range'])

    await usageApi.usageByModel('kimi', 'Month')
    expectLastCall('usage_by_model', ['platform', 'range'])

    await usageApi.usageSeries('all', 'Last5Hours')
    expectLastCall('usage_series', ['platform', 'range'])
    expect((invokeMock.mock.lastCall?.[1] as Record<string, unknown>).range).toBe('Last5Hours')
  })

  it('quotaRefresh / quotaLatest / platformAccountGet：仅 platform', async () => {
    await usageApi.quotaRefresh('kimi')
    expectLastCall('quota_refresh', ['platform'])

    await usageApi.quotaLatest('kimi')
    expectLastCall('quota_latest', ['platform'])

    await usageApi.platformAccountGet('kimi')
    expectLastCall('platform_account_get', ['platform'])
  })

  it('platformAccountSet：platform + input + apiKey，apiKey 为 camelCase', async () => {
    const input: PlatformAccountInput = {
      platform: 'kimi',
      name: '默认',
      api_base: null,
      quota_total: null,
      quota_cycle: null,
      enabled: true,
    }
    await usageApi.platformAccountSet('kimi', input, 'sk-test')
    expectLastCall('platform_account_set', ['platform', 'input', 'apiKey'])
    const args = invokeMock.mock.lastCall?.[1] as Record<string, unknown>
    expect(args.input).toBe(input)
    expect(args.apiKey).toBe('sk-test')
    expect('api_key' in args).toBe(false)

    // apiKey 省略（沿用 keyring 已有 key）
    await usageApi.platformAccountSet('kimi', input)
    expectLastCall('platform_account_set', ['platform', 'input', 'apiKey'])
  })
})

describe('api/proxy.ts invoke 契约', () => {
  it('proxyStatus / recoverSystemProxy：无参数命令', async () => {
    await proxyApi.proxyStatus()
    expect(invokeMock).toHaveBeenCalledTimes(1)
    expect(invokeMock.mock.lastCall?.[0]).toBe('proxy_status')
    expect(invokeMock.mock.lastCall?.[1]).toBeUndefined()

    await proxyApi.recoverSystemProxy()
    expect(invokeMock).toHaveBeenCalledTimes(2)
    expect(invokeMock.mock.lastCall?.[0]).toBe('recover_system_proxy')
    expect(invokeMock.mock.lastCall?.[1]).toBeUndefined()
  })

  it('selectProxy：select_proxy，group + node', async () => {
    await proxyApi.selectProxy('Proxy', '节点A')
    expectLastCall('select_proxy', ['group', 'node'])
  })

  it('testDelay：test_delay；省略 group 时显式发 null（Rust 侧 Option）', async () => {
    await proxyApi.testDelay('Proxy')
    expectLastCall('test_delay', ['group'])
    expect((invokeMock.mock.lastCall?.[1] as Record<string, unknown>).group).toBe('Proxy')

    await proxyApi.testDelay()
    expectLastCall('test_delay', ['group'])
    expect((invokeMock.mock.lastCall?.[1] as Record<string, unknown>).group).toBeNull()
  })

  it('setSystemProxy：set_system_proxy，enabled', async () => {
    await proxyApi.setSystemProxy(true)
    expectLastCall('set_system_proxy', ['enabled'])
    expect((invokeMock.mock.lastCall?.[1] as Record<string, unknown>).enabled).toBe(true)
  })

  it('switchRuleMode：switch_rule_mode，mode 为小写枚举', async () => {
    await proxyApi.switchRuleMode('global')
    expectLastCall('switch_rule_mode', ['mode'])
    expect((invokeMock.mock.lastCall?.[1] as Record<string, unknown>).mode).toBe('global')
  })
})
