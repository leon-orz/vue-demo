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
      if (!wrapper!.text().includes('魔法工具')) {
        throw new Error('异步摘要卡未挂载完成')
      }
    },
    { timeout: 5000, interval: 10 },
  )
}

describe('首页看板', () => {
  it('渲染已登记摘要卡，魔法工具卡读取代理状态', async () => {
    await openDashboard()
    // 代理状态由魔法工具卡挂载异步读取，等待 IPC 真实发生
    await vi.waitFor(
      () => {
        if (!invoke.mock.calls.some(([command]) => command === 'proxy_status')) {
          throw new Error('代理状态尚未读取')
        }
      },
      { timeout: 5000, interval: 10 },
    )
    expect(wrapper!.text()).toContain('魔法工具')
    // 规划中功能卡列出五个占位模块
    expect(wrapper!.text()).toContain('规划中功能')
    expect(wrapper!.text()).toContain('信息订阅')
    expect(wrapper!.text()).toContain('AI 网关')
    expect(wrapper!.text()).toContain('开发环境')
  })

  it('单卡 IPC 失败只降级该卡，看板整体照常渲染', async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === 'proxy_status') throw new Error('代理状态不可用')
      if (command === 'app_preferences_get') return { theme: 'system' }
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
  })
})
