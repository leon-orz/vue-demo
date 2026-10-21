import { invoke } from '@tauri-apps/api/core'
import type { NetworkDiagnostic, NetworkRepairResult } from '@toogl/core-types'

export function diagnoseNetwork(): Promise<NetworkDiagnostic> {
  return invoke('system_tools_diagnose_network')
}

export function flushDnsCache(): Promise<NetworkRepairResult> {
  return invoke('system_tools_flush_dns')
}
