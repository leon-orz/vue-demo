import { ref } from 'vue'
import type { CleanupScan, CommandError } from '@toogl/core-types'

import { toCommandError } from '@/shared/errors'
import { scanTemporaryFiles } from './api'

export function useStorageScan() {
  const scan = ref<CleanupScan | null>(null)
  const error = ref<CommandError | null>(null)
  const scanning = ref(false)

  async function run() {
    if (scanning.value) return
    scanning.value = true
    error.value = null
    try {
      scan.value = await scanTemporaryFiles()
    } catch (cause) {
      error.value = toCommandError(cause)
    } finally {
      scanning.value = false
    }
  }

  return { scan, error, scanning, run }
}
