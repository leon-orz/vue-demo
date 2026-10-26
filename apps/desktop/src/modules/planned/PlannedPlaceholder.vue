<script setup lang="ts">
import { Construction } from '@lucide/vue'
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { toolModules } from '@/app/modules'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

const route = useRoute()
const current = computed(() => toolModules.find((module) => module.route.path === route.path))
const description = computed(() => current.value?.planned ?? '该功能正在设计中。')
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold tracking-tight">{{ route.meta.title }}</h1>
      <p class="mt-1 flex items-center gap-2 text-sm text-muted-foreground">
        规划中的功能，尚未实现。
      </p>
    </header>
    <Card class="border-dashed">
      <CardHeader>
        <CardTitle class="flex items-center gap-2 text-base">
          <Construction class="size-4 text-muted-foreground" aria-hidden="true" />
          占位页面
          <Badge variant="outline">规划中</Badge>
        </CardTitle>
      </CardHeader>
      <CardContent class="flex flex-col gap-2 text-sm text-muted-foreground">
        <p>{{ description }}</p>
        <p>设计确定后此页面将被正式实现替换；能力边界见 docs/01-产品说明.md。</p>
      </CardContent>
    </Card>
  </div>
</template>
