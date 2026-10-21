import { invoke } from '@tauri-apps/api/core'
import type { SystemInformation } from '@toogl/core-types'

export function getSystemInformation(): Promise<SystemInformation> {
  return invoke('system_tools_information')
}
