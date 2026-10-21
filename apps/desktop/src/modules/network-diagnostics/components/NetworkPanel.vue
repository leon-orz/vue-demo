<script setup lang="ts">
import { Network, RefreshCw } from '@lucide/vue'
import type { CommandError, NetworkDiagnostic, NetworkRepairResult } from '@toogl/core-types'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import LoadErrorBanner from '@/shared/components/LoadErrorBanner.vue'

defineProps<{
  diagnostic: NetworkDiagnostic | null
  repair: NetworkRepairResult | null
  diagnosticError: CommandError | null
  repairError: CommandError | null
  diagnosing: boolean
  repairing: boolean
}>()

const emit = defineEmits<{ diagnose: []; repair: [] }>()
</script>

<template>
  <section class="flex flex-col gap-4">
    <Card>
      <CardHeader>
        <CardTitle class="text-base">网络诊断</CardTitle>
        <CardDescription>检查 DNS 解析及到测试站点的 TCP 连接，不修改网络设置。</CardDescription>
      </CardHeader>
      <CardContent class="flex flex-col gap-4">
        <Button class="w-fit" variant="outline" :disabled="diagnosing" @click="emit('diagnose')">
          <Network class="size-4" />{{ diagnosing ? '诊断中…' : '运行诊断' }}
        </Button>
        <LoadErrorBanner
          v-if="diagnosticError"
          :error="diagnosticError"
          @retry="emit('diagnose')"
        />
        <div v-if="diagnostic" class="space-y-2 text-sm">
          <p>DNS 解析：{{ diagnostic.dns_resolution ? '正常' : '失败或超时' }}</p>
          <p>
            TCP 连接：{{
              diagnostic.dns_resolution ? (diagnostic.tcp_connection ? '正常' : '失败') : '未执行'
            }}
          </p>
          <p class="text-muted-foreground">{{ diagnostic.message }}</p>
        </div>
      </CardContent>
    </Card>
    <Card>
      <CardHeader>
        <CardTitle class="text-base">刷新 DNS 缓存</CardTitle>
        <CardDescription>
          仅清空 Windows DNS
          客户端缓存；不会重置网卡或更改代理设置。权限不足时会显示失败，不会自动提权。
        </CardDescription>
      </CardHeader>
      <CardContent class="flex flex-col gap-3">
        <Button class="w-fit" :disabled="repairing" @click="emit('repair')">
          <RefreshCw class="size-4" :class="repairing ? 'animate-spin' : ''" />
          {{ repairing ? '正在刷新…' : '刷新 DNS 缓存' }}
        </Button>
        <LoadErrorBanner v-if="repairError" :error="repairError" @retry="emit('repair')" />
        <p v-else-if="repairing" class="text-sm text-muted-foreground">DNS 缓存刷新中…</p>
        <p
          v-if="repair"
          class="text-sm"
          :class="repair.success ? 'text-foreground' : 'text-destructive'"
        >
          {{ repair.message }}
        </p>
      </CardContent>
    </Card>
  </section>
</template>
