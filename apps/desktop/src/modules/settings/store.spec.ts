import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const api = vi.hoisted(() => ({ getAppPreferences: vi.fn(), setAppPreferences: vi.fn() }))
vi.mock('./api', () => api)
import { useSettingsStore } from './store'

const THEME_KEY = 'toogl.settings.theme'

/** 用可变 matchMedia 验证系统主题跟随及监听器清理。 */
function stubMatchMedia() {
  const listeners = new Set<() => void>()
  let dark = false
  vi.stubGlobal('matchMedia', () => ({
    get matches() {
      return dark
    },
    addEventListener: (_event: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_event: string, listener: () => void) => listeners.delete(listener),
  }))
  return {
    listeners,
    setDark(value: boolean) {
      dark = value
      listeners.forEach((listener) => listener())
    },
  }
}

beforeEach(() => {
  localStorage.clear()
  setActivePinia(createPinia())
  vi.resetAllMocks()
  api.getAppPreferences.mockResolvedValue(null)
  api.setAppPreferences.mockResolvedValue(undefined)
})

afterEach(() => {
  vi.unstubAllGlobals()
  document.documentElement.classList.remove('dark')
})

describe('软件偏好', () => {
  it('旧主题用于首屏，数据库无记录才迁移', async () => {
    stubMatchMedia()
    localStorage.setItem(THEME_KEY, 'dark')
    const store = useSettingsStore()
    const initialized = store.init()
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    await initialized
    expect(api.setAppPreferences).toHaveBeenCalledExactlyOnceWith({ theme: 'dark' })
  })

  it('数据库覆盖缓存，重复初始化不叠加监听和读取', async () => {
    const media = stubMatchMedia()
    localStorage.setItem(THEME_KEY, 'dark')
    api.getAppPreferences.mockResolvedValue({ theme: 'system' })
    const store = useSettingsStore()
    await Promise.all([store.init(), store.init()])
    await store.init()
    expect(api.getAppPreferences).toHaveBeenCalledTimes(1)
    expect(api.setAppPreferences).not.toHaveBeenCalled()
    expect(media.listeners.size).toBe(1)
    expect(localStorage.getItem(THEME_KEY)).toBe('system')
    media.setDark(true)
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    await store.setTheme('light')
    media.setDark(true)
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })

  it('保存失败不改变主题和缓存，重试后可以保存', async () => {
    stubMatchMedia()
    const store = useSettingsStore()
    await store.init()
    api.setAppPreferences.mockRejectedValueOnce(new Error('写入失败'))
    await store.setTheme('dark')
    expect(store.theme).toBe('system')
    expect(localStorage.getItem(THEME_KEY)).toBe('system')
    expect(store.error?.message).toBe('写入失败')
    await store.setTheme('dark')
    expect(store.error).toBeNull()
    expect(store.theme).toBe('dark')
    expect(localStorage.getItem(THEME_KEY)).toBe('dark')
  })

  it('读取失败不会覆盖未知的数据库偏好，缓存保留以便重试', async () => {
    stubMatchMedia()
    localStorage.setItem(THEME_KEY, 'dark')
    api.getAppPreferences.mockRejectedValue(new Error('数据库不可用'))
    const store = useSettingsStore()
    await store.setTheme('light')
    expect(api.setAppPreferences).not.toHaveBeenCalled()
    expect(store.theme).toBe('dark')
    expect(localStorage.getItem(THEME_KEY)).toBe('dark')
    api.getAppPreferences.mockResolvedValue({ theme: 'light' })
    await store.init()
    expect(store.theme).toBe('light')
    expect(store.error).toBeNull()
  })

  it('非法缓存回退到系统主题', async () => {
    stubMatchMedia()
    localStorage.setItem(THEME_KEY, 'invalid')
    const store = useSettingsStore()
    await store.init()
    expect(store.theme).toBe('system')
    expect(api.setAppPreferences).toHaveBeenCalledWith({ theme: 'system' })
  })
})
