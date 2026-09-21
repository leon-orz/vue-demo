import { defineStore } from 'pinia'
import { ref, watch } from 'vue'

export type DatasourceMode = 'direct' | 'hub' | 'gateway'
export type ThemeMode = 'light' | 'dark' | 'system'

const THEME_KEY = 'toogl.settings.theme'
const KIMI_SESSIONS_DIR_KEY = 'toogl.settings.kimiSessionsDir'

/** M6 通用偏好（S1-2：主题 + Kimi 日志目录覆盖，localStorage 落盘） */
export const useSettingsStore = defineStore('settings', () => {
  // S2-2 落地离线降级切换逻辑，当前仅占位
  const datasourceMode = ref<DatasourceMode>('direct')

  const theme = ref<ThemeMode>((localStorage.getItem(THEME_KEY) as ThemeMode | null) ?? 'system')
  /** Kimi Code 会话日志目录覆盖；留空 = 自动发现（~/.kimi-code/sessions） */
  const kimiSessionsDir = ref(localStorage.getItem(KIMI_SESSIONS_DIR_KEY) ?? '')

  /** 按当前偏好把 dark class 写到 <html> 上 */
  function applyTheme() {
    const dark =
      theme.value === 'dark' ||
      (theme.value === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
    document.documentElement.classList.toggle('dark', dark)
  }

  watch(theme, (v) => {
    localStorage.setItem(THEME_KEY, v)
    applyTheme()
  })
  watch(kimiSessionsDir, (v) => localStorage.setItem(KIMI_SESSIONS_DIR_KEY, v))

  /** 应用启动时调用一次：应用主题并跟随系统主题变化 */
  function init() {
    applyTheme()
    window
      .matchMedia('(prefers-color-scheme: dark)')
      .addEventListener('change', () => theme.value === 'system' && applyTheme())
  }

  return { datasourceMode, theme, kimiSessionsDir, init, applyTheme }
})
