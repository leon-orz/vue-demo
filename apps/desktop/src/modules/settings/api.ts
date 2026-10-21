import { invoke } from '@tauri-apps/api/core'
import type { AppPreferences } from '@toogl/core-types'

export function getAppPreferences(): Promise<AppPreferences | null> {
  return invoke('app_preferences_get')
}

export function setAppPreferences(preferences: AppPreferences): Promise<void> {
  return invoke('app_preferences_set', { preferences })
}
