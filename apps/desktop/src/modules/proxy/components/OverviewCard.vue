<script setup lang="ts">
import { WandSparkles } from '@lucide/vue'
import { computed, onMounted } from 'vue'
import { RouterLink } from 'vue-router'

import LoadErrorBanner from '@/shared/components/LoadErrorBanner.vue'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { RULE_MODE_LABELS, useProxyStore } from '../store'

const proxy = useProxyStore()
const currentNode = computed(() => {
  const selector = proxy.groups.find((group) => group.group_type === 'selector')
  const remembered = proxy.status?.remembered_selection
  if (selector) return `${selector.name} · ${selector.now}`
  return remembered ? `${remembered.group} · ${remembered.node}` : '未选择节点'
})
onMounted(() => proxy.refresh())
</script>

<template>
  <Card>
    <CardHeader>
      <RouterLink to="/proxy" class="flex items-center justify-between">
        <CardTitle class="text-base">魔法工具</CardTitle><WandSparkles class="size-4" />
      </RouterLink>
    </CardHeader>
    <CardContent class="flex flex-col gap-3">
      <LoadErrorBanner v-if="proxy.loadError" :error="proxy.loadError" @retry="proxy.refresh()" />
      <template v-if="proxy.status?.sidecar_alive">
        <div class="flex items-center gap-2">
          <Badge variant="secondary">{{
            proxy.status.rule_mode ? RULE_MODE_LABELS[proxy.status.rule_mode] : '未知模式'
          }}</Badge>
          <Badge :variant="proxy.status.system_proxy?.enabled ? 'default' : 'outline'"
            >系统代理{{ proxy.status.system_proxy?.enabled ? '开' : '关' }}</Badge
          >
        </div>
        <p class="truncate text-sm text-muted-foreground">{{ currentNode }}</p>
      </template>
      <p v-else class="text-sm text-muted-foreground">
        {{ proxy.status?.startup_error ? '内核启动失败' : '内核未运行或正在读取状态' }}
      </p>
      <RouterLink
        v-if="proxy.status?.startup_error || proxy.leftoverVisible"
        to="/proxy"
        class="text-sm text-primary hover:underline"
        >{{ proxy.leftoverVisible ? '系统代理有残留，前往处理' : '查看代理问题' }}</RouterLink
      >
    </CardContent>
  </Card>
</template>
