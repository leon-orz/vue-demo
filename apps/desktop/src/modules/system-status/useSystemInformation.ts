import { computed, ref } from 'vue'
import type { CommandError, SystemInformation } from '@toogl/core-types'

import { toCommandError } from '@/shared/errors'
import { getSystemInformation } from './api'

export function useSystemInformation() {
  const information = ref<SystemInformation | null>(null)
  const error = ref<CommandError | null>(null)
  const loading = ref(false)
  const systemDriveUsage = computed(() => {
    if (!information.value || information.value.system_drive_total_bytes === 0n) return null
    return (
      Number(information.value.system_drive_free_bytes) /
      Number(information.value.system_drive_total_bytes)
    )
  })

  async function load() {
    if (loading.value) return
    loading.value = true
    error.value = null
    try {
      information.value = await getSystemInformation()
    } catch (cause) {
      error.value = toCommandError(cause)
    } finally {
      loading.value = false
    }
  }

  return { information, error, loading, systemDriveUsage, load }
}
