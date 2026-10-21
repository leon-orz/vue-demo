import { ref } from 'vue'
import type { CommandError, NetworkDiagnostic, NetworkRepairResult } from '@toogl/core-types'

import { toCommandError } from '@/shared/errors'
import { diagnoseNetwork, flushDnsCache } from './api'

export function useNetworkDiagnostics() {
  const diagnostic = ref<NetworkDiagnostic | null>(null)
  const repair = ref<NetworkRepairResult | null>(null)
  const diagnosticError = ref<CommandError | null>(null)
  const repairError = ref<CommandError | null>(null)
  const diagnosing = ref(false)
  const flushingDns = ref(false)

  async function diagnose() {
    if (diagnosing.value) return
    diagnosing.value = true
    diagnosticError.value = null
    try {
      diagnostic.value = await diagnoseNetwork()
    } catch (cause) {
      diagnosticError.value = toCommandError(cause)
    } finally {
      diagnosing.value = false
    }
  }

  async function flushDns() {
    if (flushingDns.value) return
    flushingDns.value = true
    repairError.value = null
    repair.value = null
    try {
      repair.value = await flushDnsCache()
    } catch (cause) {
      repairError.value = toCommandError(cause)
    } finally {
      flushingDns.value = false
    }
  }

  return {
    diagnostic,
    repair,
    diagnosticError,
    repairError,
    diagnosing,
    flushingDns,
    diagnose,
    flushDns,
  }
}
