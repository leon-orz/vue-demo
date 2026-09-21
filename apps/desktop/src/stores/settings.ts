import { defineStore } from 'pinia'
import { ref, watch } from 'vue'

// TODO(S2)：偏好迁移到 get_settings/set_setting 命令（SQLite）——当前 localStorage，清 WebView 数据会丢

export type DatasourceMode = 'direct' | 'hub' | 'gateway'
export type ThemeMode = 'light' | 'dark' | 'system'

const THEME_KEY = 'toogl.settings.theme'
const KIMI_SESSIONS_DIR_KEY = 'toogl.settings.kimiSessionsDir'

/**
 * 系统主题监听器的注销函数（模块级，HMR/重复 init 时先注销旧监听，避免叠加）。
 * 监听器挂在 window 上，store 重建不会自动清理，必须手动管理。
 */
let disposeSystemThemeListener: (() => void) | null = null

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
    disposeSystemThemeListener?.()
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const onSystemThemeChange = () => {
      if (theme.value === 'system') applyTheme()
    }
    media.addEventListener('change', onSystemThemeChange)
    disposeSystemThemeListener = () => media.removeEventListener('change', onSystemThemeChange)
  }

  return { datasourceMode, theme, kimiSessionsDir, init, applyTheme }
})
