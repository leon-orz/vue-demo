import { invoke } from '@tauri-apps/api/core'
import type { CleanupScan } from '@toogl/core-types'

export function scanTemporaryFiles(): Promise<CleanupScan> {
  return invoke('system_tools_scan_cleanup')
}
