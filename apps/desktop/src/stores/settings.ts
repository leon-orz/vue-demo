import { defineStore } from 'pinia'
import { ref } from 'vue'

export type DatasourceMode = 'direct' | 'hub' | 'gateway'

export const useSettingsStore = defineStore('settings', () => {
  // S1-1 仅占位；S2-2 落地离线降级切换逻辑
  const datasourceMode = ref<DatasourceMode>('direct')

  return { datasourceMode }
})
