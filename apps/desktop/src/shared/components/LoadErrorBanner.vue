<script setup lang="ts">
import type { CommandError } from '@/shared/errors'
import { Button } from '@/components/ui/button'

/** 通用错误横幅只负责重试；配置入口由所属模块提供。 */
defineProps<{ error: CommandError }>()
const emit = defineEmits<{ retry: [] }>()
</script>

<template>
  <div
    class="flex items-center justify-between gap-4 rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3 text-sm"
    role="alert"
  >
    <span class="text-destructive">
      <template v-if="error.kind === 'unauthorized'">凭据失效，请检查该工具的配置。</template>
      <template v-else-if="error.kind === 'network'">网络问题，可重试。</template>
      <template v-else>数据加载失败：{{ error.message }}</template>
    </span>
    <Button variant="outline" size="sm" class="shrink-0" @click="emit('retry')">重试</Button>
  </div>
</template>
