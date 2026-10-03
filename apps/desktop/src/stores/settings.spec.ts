import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'

import { useSettingsStore } from './settings'

const THEME_KEY = 'toogl.settings.theme'
const KIMI_SESSIONS_DIR_KEY = 'toogl.settings.kimiSessionsDir'

type ChangeListener = () => void

/**
 * jsdom 未实现 window.matchMedia，这里整体打桩：
 * matches 可翻转，add/removeEventListener 用 Set 记录活跃监听器（便于断言叠加/注销）
 */
function stubMatchMedia(initialDark: boolean) {
  const state = { dark: initialDark, listeners: new Set<ChangeListener>() }
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({
      get matches() {
        return state.dark
      },
      addEventListener: (_event: string, cb: ChangeListener) => state.listeners.add(cb),
      removeEventListener: (_event: string, cb: ChangeListener) => state.listeners.delete(cb),
    })),
  )
  return {
    /** 翻转系统主题并派发 change（模拟用户在系统设置里切换深浅色） */
    setDark(dark: boolean) {
      state.dark = dark
      state.listeners.forEach((cb) => cb())
    },
    activeListenerCount: () => state.listeners.size,
  }
}

beforeEach(() => {
  localStorage.clear()
  setActivePinia(createPinia())
})

afterEach(() => {
  vi.unstubAllGlobals()
  document.documentElement.classList.remove('dark')
})

describe('applyTheme 对 document 的实际效果', () => {
  it('dark 加 dark class，light 移除', () => {
    stubMatchMedia(false)
    const store = useSettingsStore()

    store.theme = 'dark'
    store.applyTheme()
    expect(document.documentElement.classList.contains('dark')).toBe(true)

    store.theme = 'light'
    store.applyTheme()
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })

  it('system 模式跟随 matchMedia 的 matches', () => {
    const media = stubMatchMedia(true)
    const store = useSettingsStore()
    store.theme = 'system'

    store.applyTheme()
    expect(document.documentElement.classList.contains('dark')).toBe(true)

    media.setDark(false)
    store.applyTheme()
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })
})

describe('系统主题监听器', () => {
  it('init 注册 change 监听，system 模式下系统主题变化自动重应用', () => {
    const media = stubMatchMedia(false)
    const store = useSettingsStore()
    store.theme = 'system'
    store.init()

    expect(media.activeListenerCount()).toBe(1)
    expect(document.documentElement.classList.contains('dark')).toBe(false)

    media.setDark(true)
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })

  it('非 system 模式下系统主题变化不影响界面', () => {
    const media = stubMatchMedia(false)
    const store = useSettingsStore()
    store.theme = 'light'
    store.init()

    media.setDark(true)
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })

  it('重复 init 注销旧监听，不叠加（易回归点）', () => {
    const media = stubMatchMedia(false)
    const store = useSettingsStore()
    store.theme = 'system'

    store.init()
    store.init()
    store.init()
    // 监听器挂在 window 上，store 重建/重复 init 必须手动注销旧的，否则响应会翻倍
    expect(media.activeListenerCount()).toBe(1)

    // 系统主题变化只触发一次重应用（若叠加监听，行为虽同但监听数会泄漏增长）
    media.setDark(true)
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(media.activeListenerCount()).toBe(1)
  })
})

describe('localStorage 持久化', () => {
  it('切换主题写入 localStorage，新 store 读取到持久化值', async () => {
    stubMatchMedia(false)
    const store = useSettingsStore()
    store.theme = 'dark'
    await nextTick()
    expect(localStorage.getItem(THEME_KEY)).toBe('dark')

    setActivePinia(createPinia())
    const reloaded = useSettingsStore()
    expect(reloaded.theme).toBe('dark')
  })

  it('主题变化经 watch 自动重应用 dark class', async () => {
    stubMatchMedia(false)
    const store = useSettingsStore()
    store.theme = 'dark'
    await nextTick()
    expect(document.documentElement.classList.contains('dark')).toBe(true)
  })

  it('脏数据兜底：非法主题值回退 system，不原样进入状态', () => {
    localStorage.setItem(THEME_KEY, 'neon')
    stubMatchMedia(false)
    const store = useSettingsStore()
    expect(store.theme).toBe('system')
  })

  it('kimiSessionsDir 修改后持久化，新 store 读取到', async () => {
    stubMatchMedia(false)
    const store = useSettingsStore()
    store.kimiSessionsDir = 'D:/logs/kimi'
    await nextTick()
    expect(localStorage.getItem(KIMI_SESSIONS_DIR_KEY)).toBe('D:/logs/kimi')

    setActivePinia(createPinia())
    const reloaded = useSettingsStore()
    expect(reloaded.kimiSessionsDir).toBe('D:/logs/kimi')
  })
})
