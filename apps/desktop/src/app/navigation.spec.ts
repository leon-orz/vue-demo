import { createPinia } from 'pinia'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, RouterView } from 'vue-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
const invoke = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
import { router as appRouter } from './router'

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
      case 'subscription_status':
        return {
          url: 'https://example.com/sub',
          node_count: 1,
          manual_count: 0,
          last_updated: null,
          last_error: null,
          updating: false,
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

async function open(path: string) {
  const router = createRouter({ history: createMemoryHistory(), routes: appRouter.options.routes })
  await router.push(path)
  await router.isReady()
  wrapper = mount(RouterView, { global: { plugins: [createPinia(), router] } })
  await flushPromises()
  return router
}

describe('模块入口', () => {
  it('规划中模块路由渲染占位页，不发起业务 IPC', async () => {
    for (const path of [
      '/feed',
      '/ai-gateway',
      '/system-tuning',
      '/network-tuning',
      '/dev-environment',
    ]) {
      await open(path)
      expect(wrapper!.text()).toContain('占位页面')
      expect(wrapper!.text()).toContain('规划中')
    }
    expect(invoke).not.toHaveBeenCalled()
  })

  it('软件设置只加载软件偏好，不读取业务账户或日志目录', async () => {
    await open('/settings')
    expect(wrapper!.text()).toContain('关于 Toogl')
    expect(invoke.mock.calls.map(([command]) => command)).toEqual(['app_preferences_get'])
  })

  it('代理配置再次进入仍回填已保存链接，状态刷新不覆盖正在编辑的链接', async () => {
    const router = await open('/proxy?tab=config&source=dashboard')
    const input = () => wrapper!.find('input')
    expect((input().element as HTMLInputElement).value).toBe('https://example.com/sub')
    await input().setValue('https://example.com/edited')
    // 模拟再次同步相同订阅状态，不能覆盖用户尚未保存的输入。
    const { useSubscriptionStore } = await import('@/modules/proxy/subscription')
    await useSubscriptionStore().refresh()
    await flushPromises()
    expect((input().element as HTMLInputElement).value).toBe('https://example.com/edited')

    const proxyTab = wrapper!.findAll('[role="tab"]').find((button) => button.text() === '代理')!
    await proxyTab.trigger('mousedown', { button: 0, ctrlKey: false })
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ source: 'dashboard' })
    expect(wrapper!.find('input').exists()).toBe(false)

    const configure = wrapper!
      .findAll('button')
      .find((button) => button.text() === '配置订阅与导入节点')!
    await configure.trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ tab: 'config', source: 'dashboard' })
    expect((input().element as HTMLInputElement).value).toBe('https://example.com/sub')
  })
})
