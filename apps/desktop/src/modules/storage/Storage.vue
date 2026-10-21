<script setup lang="ts">
import { computed, onMounted } from 'vue'

import CleanupPanel from './components/CleanupPanel.vue'
import OptimizationPanel from './components/OptimizationPanel.vue'
import { useStorageScan } from './useStorageScan'
import { useSystemInformation } from '@/modules/system-status/useSystemInformation'

const { systemDriveUsage, load } = useSystemInformation()
const { scan, error, scanning, run } = useStorageScan()
const hasSystemDriveUsage = computed(() => systemDriveUsage.value !== null)
onMounted(() => void load())
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold">空间维护</h1>
      <p class="mt-1 text-sm text-muted-foreground">查看系统盘空间并安全扫描当前用户临时目录。</p>
    </header>
    <OptimizationPanel v-if="hasSystemDriveUsage" :system-drive-usage="systemDriveUsage" />
    <CleanupPanel :scan="scan" :error="error" :scanning="scanning" @scan="run" />
  </div>
</template>
