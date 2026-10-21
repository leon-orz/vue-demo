<script setup lang="ts">
import { HardDrive, ScanSearch } from '@lucide/vue'
import type { CleanupScan, CommandError } from '@toogl/core-types'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import LoadErrorBanner from '@/shared/components/LoadErrorBanner.vue'
import { formatBytes } from '@/lib/formatBytes'

defineProps<{
  scan: CleanupScan | null
  error: CommandError | null
  scanning: boolean
}>()

const emit = defineEmits<{ scan: [] }>()
</script>

<template>
  <section class="flex flex-col gap-4">
    <Card>
      <CardHeader>
        <CardTitle class="flex items-center gap-2 text-base">
          <HardDrive class="size-4" />临时文件扫描
        </CardTitle>
        <CardDescription>只扫描当前用户临时目录，不会删除或修改任何文件。</CardDescription>
      </CardHeader>
      <CardContent class="flex flex-col gap-4">
        <Button class="w-fit" :disabled="scanning" @click="emit('scan')">
          <ScanSearch class="size-4" :class="scanning ? 'animate-pulse' : ''" />
          {{ scanning ? '扫描中…' : '扫描临时文件' }}
        </Button>
        <LoadErrorBanner v-if="error" :error="error" @retry="emit('scan')" />
        <div v-if="scan" class="grid grid-cols-1 gap-4 sm:grid-cols-3">
          <div>
            <p class="text-sm text-muted-foreground">文件数量</p>
            <p class="mt-1 text-lg font-semibold">{{ scan.files.toLocaleString() }}</p>
          </div>
          <div>
            <p class="text-sm text-muted-foreground">占用空间</p>
            <p class="mt-1 text-lg font-semibold">{{ formatBytes(scan.bytes) }}</p>
          </div>
          <div>
            <p class="text-sm text-muted-foreground">跳过项目</p>
            <p class="mt-1 text-lg font-semibold">{{ scan.skipped_entries.toLocaleString() }}</p>
          </div>
        </div>
      </CardContent>
    </Card>
  </section>
</template>
