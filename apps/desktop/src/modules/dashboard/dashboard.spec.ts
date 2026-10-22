import { createPinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, RouterView } from 'vue-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
const invoke = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
import { router as appRouter } from '@/app/router'

let wrapper: VueWrapper | undefined

beforeEach(() => {
  localStorage.clear()
  vi.stubGlobal('matchMedia', () => ({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }))
  invoke.mockReset().mockImplementation(async (command: string) => {
    switch (command) {
      case 'app_preferences_get':
        return { theme: 'system' }
      case 'proxy_status':
        return {
          sidecar_alive: true,
          proxies: { groups: [] },
          rule_mode: 'rule',
          system_proxy: { enabled: false, server: null },
        }
      case 'system_tools_information':
        return {
          windows_edition: 'Windows 11',
          version: '24H2',
          architecture: 'x64',
          computer_name: '看板设备',
          processor: '看板处理器',
          total_memory_bytes: 16_000_000_000n,
          system_drive_free_bytes: 50_000_000_000n,
          system_drive_total_bytes: 100_000_000_000n,
        }
      default:
        return []
    }
  })
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  vi.unstubAllGlobals()
})

async function openDashboard() {
  const router = createRouter({ history: createMemoryHistory(), routes: appRouter.options.routes })
  await router.push('/dashboard')
  await router.isReady()
  wrapper = mount(RouterView, { global: { plugins: [createPinia(), router] } })
  await flushPromises()
  // defineAsyncComponent 的加载 promise 链需要真实时间轮转才能完成挂载
  await vi.waitFor(
    () => {
      if (!wrapper!.text().includes('固定测试目标')) {
        throw new Error('异步摘要卡未挂载完成')
      }
    },
    { timeout: 5000, interval: 10 },
  )
}

describe('首页看板', () => {
  it('渲染全部已登记摘要卡，系统状态卡读取只读信息，不自动诊断/扫描', async () => {
    await openDashboard()
    // 系统状态/空间维护两张卡各自挂载读取一次系统信息；等待第二张卡完成
    await vi.waitFor(
      () => {
        const calls = invoke.mock.calls.filter(
          ([command]) => command === 'system_tools_information',
        )
        if (calls.length < 2) throw new Error('系统信息尚未读取两卡各一次')
      },
      { timeout: 5000, interval: 10 },
    )
    const text = wrapper!.text()
    for (const title of ['魔法工具', '网络诊断', '系统状态', '空间维护']) {
      expect(text).toContain(title)
    }
    // 系统状态与空间维护卡挂载读取系统信息（同一 IPC，各一次）
    const informationCalls = invoke.mock.calls.filter(
      ([command]) => command === 'system_tools_information',
    )
    expect(informationCalls.length).toBe(2)
    // 诊断/扫描是主动操作，看板不自动执行
    expect(invoke.mock.calls.some(([command]) => command === 'system_tools_diagnose_network')).toBe(
      false,
    )
    expect(invoke.mock.calls.some(([command]) => command === 'system_tools_scan_cleanup')).toBe(
      false,
    )
  })

  it('单卡 IPC 失败只降级该卡，其余卡片照常渲染', async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === 'system_tools_information') throw new Error('系统信息不可用')
      if (command === 'app_preferences_get') return { theme: 'system' }
      if (command === 'proxy_status')
        return {
          sidecar_alive: true,
          proxies: { groups: [] },
          rule_mode: 'rule',
          system_proxy: { enabled: false, server: null },
        }
      return []
    })
    await openDashboard()
    // 等待失败卡的错误横幅实际渲染出来再断言
    await vi.waitFor(
      () => {
        if (!wrapper!.text().includes('数据加载失败')) throw new Error('错误横幅未渲染')
      },
      { timeout: 5000, interval: 10 },
    )
    const text = wrapper!.text()
    // 系统状态/空间维护卡显示各自错误，魔法工具卡不受影响
    expect(text).toContain('数据加载失败')
    expect(text).toContain('魔法工具')
    expect(text).not.toContain('看板设备')
  })
})
