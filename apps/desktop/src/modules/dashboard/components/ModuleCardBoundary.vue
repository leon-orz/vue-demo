<script setup lang="ts">
import { onErrorCaptured, ref } from 'vue'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

/** 看板卡片级边界：异步组件加载失败或渲染抛错时只降级当前卡，其余卡片继续渲染。 */
const props = defineProps<{ title: string }>()

/** 重试计数：变化即销毁重建卡内容，defineAsyncComponent 随之重新加载 */
const retryCount = ref(0)
const failure = ref<string | null>(null)

onErrorCaptured((err) => {
  console.error(`看板卡片「${props.title}」渲染失败`, err)
  failure.value = err instanceof Error ? err.message : String(err)
  return false
})

function retry() {
  failure.value = null
  retryCount.value++
}
</script>

<template>
  <Card>
    <CardHeader
      ><CardTitle class="text-base">{{ title }}</CardTitle></CardHeader
    >
    <CardContent class="flex flex-col gap-3">
      <template v-if="failure">
        <p class="text-sm text-destructive" role="alert">模块摘要加载失败：{{ failure }}</p>
        <Button variant="outline" size="sm" class="w-fit" @click="retry">重试</Button>
      </template>
      <div v-else :key="retryCount" class="flex flex-col gap-3">
        <slot />
      </div>
    </CardContent>
  </Card>
</template>
