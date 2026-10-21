<script setup lang="ts">
import { RefreshCw } from '@lucide/vue'
import type { CommandError, SystemInformation } from '@toogl/core-types'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import LoadErrorBanner from '@/shared/components/LoadErrorBanner.vue'
import { formatBytes } from '@/lib/formatBytes'

defineProps<{
  information: SystemInformation | null
  error: CommandError | null
  loading: boolean
}>()

const emit = defineEmits<{ refresh: [] }>()
</script>

<template>
  <section class="flex flex-col gap-4">
    <LoadErrorBanner v-if="error" :error="error" @retry="emit('refresh')" />
    <div class="flex justify-end">
      <Button variant="outline" :disabled="loading" @click="emit('refresh')">
        <RefreshCw class="size-4" :class="loading ? 'animate-spin' : ''" />刷新
      </Button>
    </div>
    <div v-if="information" class="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <Card>
        <CardHeader><CardTitle class="text-base">操作系统</CardTitle></CardHeader>
        <CardContent class="grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)] gap-x-4 gap-y-3 text-sm">
          <span class="text-muted-foreground">版本</span>
          <span>{{ information.windows_edition }} {{ information.version }}</span>
          <span class="text-muted-foreground">架构</span><span>{{ information.architecture }}</span>
          <span class="text-muted-foreground">设备名</span>
          <span class="break-all">{{ information.computer_name }}</span>
        </CardContent>
      </Card>
      <Card>
        <CardHeader><CardTitle class="text-base">硬件与存储</CardTitle></CardHeader>
        <CardContent class="grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)] gap-x-4 gap-y-3 text-sm">
          <span class="text-muted-foreground">处理器</span><span>{{ information.processor }}</span>
          <span class="text-muted-foreground">物理内存</span>
          <span>{{ formatBytes(information.total_memory_bytes) }}</span>
          <span class="text-muted-foreground">系统盘剩余</span>
          <span>
            {{ formatBytes(information.system_drive_free_bytes) }} /
            {{ formatBytes(information.system_drive_total_bytes) }}
          </span>
        </CardContent>
      </Card>
    </div>
    <p v-else-if="loading" class="text-sm text-muted-foreground">正在读取系统信息…</p>
  </section>
</template>
