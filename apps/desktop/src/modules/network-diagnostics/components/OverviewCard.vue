<script setup lang="ts">
import { Network } from '@lucide/vue'
import { RouterLink } from 'vue-router'

import LoadErrorBanner from '@/shared/components/LoadErrorBanner.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useNetworkDiagnostics } from '../useNetworkDiagnostics'

// 卡片持有自己的会话内结果，事实源与页面同为同一条诊断 IPC；挂载不自动诊断
const { diagnostic, diagnosticError, diagnosing, diagnose } = useNetworkDiagnostics()
</script>

<template>
  <Card>
    <CardHeader>
      <RouterLink to="/network-diagnostics" class="flex items-center justify-between">
        <CardTitle class="text-base">网络诊断</CardTitle><Network class="size-4" />
      </RouterLink>
    </CardHeader>
    <CardContent class="flex flex-col gap-3">
      <LoadErrorBanner v-if="diagnosticError" :error="diagnosticError" @retry="diagnose()" />
      <template v-if="diagnostic">
        <div class="flex items-center gap-2">
          <Badge :variant="diagnostic.dns_resolution ? 'default' : 'destructive'">
            DNS{{ diagnostic.dns_resolution ? '正常' : '失败' }}
          </Badge>
          <Badge
            v-if="diagnostic.dns_resolution"
            :variant="diagnostic.tcp_connection ? 'default' : 'outline'"
          >
            TCP{{ diagnostic.tcp_connection ? '正常' : '失败' }}
          </Badge>
        </div>
        <p class="truncate text-sm text-muted-foreground">{{ diagnostic.message }}</p>
      </template>
      <p v-else class="text-sm text-muted-foreground">
        {{ diagnosing ? '诊断中…' : '尚未运行诊断，结果仅描述固定测试目标。' }}
      </p>
      <Button v-if="!diagnosing" variant="outline" size="sm" class="w-fit" @click="diagnose()">
        运行诊断
      </Button>
    </CardContent>
  </Card>
</template>
