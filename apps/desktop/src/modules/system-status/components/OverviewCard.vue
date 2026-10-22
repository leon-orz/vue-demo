<script setup lang="ts">
import { Cpu } from '@lucide/vue'
import { onMounted } from 'vue'
import { RouterLink } from 'vue-router'

import LoadErrorBanner from '@/shared/components/LoadErrorBanner.vue'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { formatBytes } from '@/lib/formatBytes'
import { useSystemInformation } from '../useSystemInformation'

// 挂载即读取一次只读系统信息，与详情页同一条 IPC；不轮询
const { information, error, loading, load } = useSystemInformation()
onMounted(() => void load())
</script>

<template>
  <Card>
    <CardHeader>
      <RouterLink to="/system-status" class="flex items-center justify-between">
        <CardTitle class="text-base">系统状态</CardTitle><Cpu class="size-4" />
      </RouterLink>
    </CardHeader>
    <CardContent class="flex flex-col gap-3">
      <LoadErrorBanner v-if="error" :error="error" @retry="load()" />
      <template v-if="information">
        <div class="flex items-center gap-2">
          <Badge variant="secondary">{{ information.windows_edition }}</Badge>
          <Badge variant="outline">{{ information.architecture }}</Badge>
        </div>
        <p class="truncate text-sm text-muted-foreground">{{ information.processor }}</p>
        <p class="text-sm text-muted-foreground">
          内存 {{ formatBytes(information.total_memory_bytes) }} · 系统盘剩余
          {{ formatBytes(information.system_drive_free_bytes) }}
        </p>
      </template>
      <p v-else-if="loading" class="text-sm text-muted-foreground">正在读取系统信息…</p>
    </CardContent>
  </Card>
</template>
