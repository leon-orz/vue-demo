<script setup lang="ts">
import { useRoute } from 'vue-router'

import { Badge } from '@/components/ui/badge'

// 未交付模块的统一占位页（标题/里程碑取自路由 meta）
const route = useRoute()
</script>

<template>
  <div class="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
    <h1 class="text-2xl font-bold tracking-tight">{{ route.meta.title }}</h1>
    <Badge variant="outline">{{ route.meta.milestone }}</Badge>
    <p class="text-sm text-muted-foreground">该模块尚未交付，敬请期待。</p>
  </div>
</template>
