<script setup lang="ts">
import { RouterLink } from 'vue-router'

import type { CommandError } from '@/api/errors'
import { Button } from '@/components/ui/button'

/** 数据加载失败横幅：unauthorized 引导去设置页，network/其余提示重试 */
defineProps<{ error: CommandError }>()
const emit = defineEmits<{ retry: [] }>()
</script>

<template>
  <div
    class="flex items-center justify-between gap-4 rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3 text-sm"
    role="alert"
  >
    <span class="text-destructive">
      <template v-if="error.kind === 'unauthorized'">凭据失效，请去设置页重新配置。</template>
      <template v-else-if="error.kind === 'network'">网络问题，可重试。</template>
      <template v-else>数据加载失败：{{ error.message }}</template>
    </span>
    <RouterLink v-if="error.kind === 'unauthorized'" to="/settings" class="shrink-0">
      <Button variant="outline" size="sm">去设置页</Button>
    </RouterLink>
    <Button v-else variant="outline" size="sm" class="shrink-0" @click="emit('retry')">重试</Button>
  </div>
</template>
