import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { ThemeMode } from '@toogl/core-types'

import { toCommandError, type CommandError } from '@/shared/errors'
import * as api from './api'

// localStorage 仅缓存主题以避免首屏闪烁；SQLite 是偏好的事实源。
const THEME_KEY = 'toogl.settings.theme'
const THEMES: readonly ThemeMode[] = ['light', 'dark', 'system']

// 缓存不是事实源：WebView 禁止存储或配额耗尽时仍必须能够启动、读取 SQLite。
function readCachedTheme(): string | null {
  try {
    return localStorage.getItem(THEME_KEY)
  } catch {
    return null
  }
}

function cacheTheme(value: ThemeMode) {
  try {
    localStorage.setItem(THEME_KEY, value)
  } catch {
    // 仅丢失下次启动的首屏加速；数据库保存结果不受影响。
  }
}

function cachedTheme(): ThemeMode {
  const value = readCachedTheme()
  return THEMES.includes(value as ThemeMode) ? (value as ThemeMode) : 'system'
}

let disposeSystemThemeListener: (() => void) | null = null

/** 软件偏好只管理全局行为，不依赖业务模块。 */
export const useSettingsStore = defineStore('settings', () => {
  const theme = ref<ThemeMode>(cachedTheme())
  const error = ref<CommandError | null>(null)
  const saving = ref(false)
  let initialized = false
  let initializing: Promise<void> | null = null

  function applyTheme() {
    const dark =
      theme.value === 'dark' ||
      (theme.value === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
    document.documentElement.classList.toggle('dark', dark)
  }

  async function load() {
    error.value = null
    try {
      const saved = await api.getAppPreferences()
      if (saved) theme.value = saved.theme
      else if (readCachedTheme() !== null) {
        // 仅数据库无记录时迁移旧值，避免缓存覆盖已有偏好。
        await api.setAppPreferences({ theme: theme.value })
      }
      cacheTheme(theme.value)
      applyTheme()
      initialized = true
    } catch (err) {
      error.value = toCommandError(err)
    }
  }

  function init(): Promise<void> {
    applyTheme()
    disposeSystemThemeListener?.()
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => {
      if (theme.value === 'system') applyTheme()
    }
    media.addEventListener('change', onChange)
    disposeSystemThemeListener = () => media.removeEventListener('change', onChange)
    if (initialized && !error.value) return Promise.resolve()
    initializing ??= load().finally(() => {
      initializing = null
    })
    return initializing
  }

  async function setTheme(value: ThemeMode) {
    if (saving.value) return
    saving.value = true
    try {
      await init()
      if (error.value) return
      await api.setAppPreferences({ theme: value })
      theme.value = value
      cacheTheme(value)
      applyTheme()
    } catch (err) {
      error.value = toCommandError(err)
    } finally {
      saving.value = false
    }
  }

  return { theme, error, saving, init, setTheme }
})
