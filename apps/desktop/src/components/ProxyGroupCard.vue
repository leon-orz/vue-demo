<script setup lang="ts">
import { Check, ChevronDown, Gauge } from '@lucide/vue'
import { computed, ref } from 'vue'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { delayToneClass, formatDelay } from '@/lib/format'
import { GROUP_TYPE_LABELS, useProxyStore } from '@/stores/proxy'
import type { ProxyGroup } from '@toogl/core-types'

/** 单个代理组卡片：组名/类型/当前节点 + 可展开节点列表 + 组级测速 */
const props = defineProps<{ group: ProxyGroup }>()

const proxy = useProxyStore()

const expanded = ref(true)
/** 只有 selector 组允许手动切换节点；urltest 等自动组只读展示 */
const selectable = computed(() => props.group.group_type === 'selector')
const testing = computed(() => proxy.testingGroups[props.group.name] === true || proxy.testingAll)

function onSelect(nodeName: string) {
  if (!selectable.value || nodeName === props.group.now) return
  void proxy.selectNode(props.group.name, nodeName)
}
</script>

<template>
  <Card>
    <CardHeader class="flex-row items-center gap-3 space-y-0">
      <button
        type="button"
        class="flex min-w-0 flex-1 items-center gap-2 text-left"
        @click="expanded = !expanded"
      >
        <ChevronDown
          class="size-4 shrink-0 text-muted-foreground transition-transform"
          :class="expanded ? '' : '-rotate-90'"
        />
        <CardTitle class="truncate text-base">{{ group.name }}</CardTitle>
        <Badge variant="secondary">{{
          GROUP_TYPE_LABELS[group.group_type] ?? group.group_type
        }}</Badge>
        <span class="truncate text-sm text-muted-foreground">当前：{{ group.now || '—' }}</span>
      </button>
      <Button
        variant="outline"
        size="sm"
        class="shrink-0"
        :disabled="testing"
        @click="proxy.testDelay(group.name)"
      >
        <Gauge class="size-4" :class="testing ? 'animate-pulse' : ''" />
        {{ testing ? '测速中…' : '测速' }}
      </Button>
    </CardHeader>
    <CardContent v-if="expanded" class="pt-0">
      <ul class="divide-y">
        <li v-for="node in group.nodes" :key="node.name">
          <button
            type="button"
            class="flex w-full items-center gap-3 rounded-md px-2 py-2 text-left text-sm transition-colors"
            :class="[
              selectable ? 'hover:bg-accent/50' : 'cursor-default',
              node.name === group.now ? 'bg-accent/60' : '',
            ]"
            :disabled="!selectable || proxy.selecting[group.name] === true"
            @click="onSelect(node.name)"
          >
            <Check
              class="size-4 shrink-0"
              :class="node.name === group.now ? 'text-primary' : 'text-transparent'"
            />
            <span class="min-w-0 flex-1 truncate">{{ node.name }}</span>
            <span class="text-xs text-muted-foreground">{{ node.node_type }}</span>
            <span
              class="w-20 shrink-0 text-right font-mono text-xs"
              :class="delayToneClass(node.delay_ms, node.delay_status)"
            >
              {{ formatDelay(node.delay_ms, node.delay_status) }}
            </span>
          </button>
        </li>
      </ul>
    </CardContent>
  </Card>
</template>
