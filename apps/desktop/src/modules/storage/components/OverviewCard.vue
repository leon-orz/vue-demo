<script setup lang="ts">
import { HardDrive } from '@lucide/vue'
import { computed, onMounted } from 'vue'
import { RouterLink } from 'vue-router'

import LoadErrorBanner from '@/shared/components/LoadErrorBanner.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { formatBytes } from '@/lib/formatBytes'
import { useStorageScan } from '../useStorageScan'
import { useSystemInformation } from '@/modules/system-status/useSystemInformation'

// 盘空间是只读事实，挂载读取一次；Temp 扫描是主动操作，卡片不自动扫描
const { information, error: infoError, load } = useSystemInformation()
const { scan, error: scanError, scanning, run } = useStorageScan()
onMounted(() => void load())

const driveUsagePercent = computed(() => {
  const info = information.value
  if (!info || info.system_drive_total_bytes === 0n) return null
  return Math.round(
    (1 - Number(info.system_drive_free_bytes) / Number(info.system_drive_total_bytes)) * 100,
  )
})
</script>

<template>
  <Card>
    <CardHeader>
      <RouterLink to="/storage" class="flex items-center justify-between">
        <CardTitle class="text-base">空间维护</CardTitle><HardDrive class="size-4" />
      </RouterLink>
    </CardHeader>
    <CardContent class="flex flex-col gap-3">
      <LoadErrorBanner v-if="infoError" :error="infoError" @retry="load()" />
      <div class="flex items-center gap-2">
        <Badge v-if="driveUsagePercent !== null" variant="secondary">
          系统盘已用 {{ driveUsagePercent }}%
        </Badge>
        <Badge v-else variant="outline">系统盘用量未知</Badge>
      </div>
      <LoadErrorBanner v-if="scanError" :error="scanError" @retry="run()" />
      <p v-if="scan" class="text-sm text-muted-foreground">
        临时文件 {{ scan.files.toLocaleString() }} 个，占用 {{ formatBytes(scan.bytes)
        }}<template v-if="scan.skipped_entries > 0n">
          ，跳过 {{ scan.skipped_entries.toLocaleString() }} 项</template
        >
      </p>
      <p v-else class="text-sm text-muted-foreground">
        {{ scanning ? '扫描中…' : '临时目录尚未扫描，只统计不删除文件。' }}
      </p>
      <Button v-if="!scanning" variant="outline" size="sm" class="w-fit" @click="run()">
        扫描临时文件
      </Button>
    </CardContent>
  </Card>
</template>
