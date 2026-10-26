<script setup lang="ts">
import { Construction } from '@lucide/vue'
import { RouterLink } from 'vue-router'
import { plannedModules } from '@/app/modules'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
</script>

<template>
  <Card class="border-dashed bg-muted/30">
    <CardHeader>
      <CardTitle class="flex items-center gap-2 text-base">
        <Construction class="size-4 text-muted-foreground" aria-hidden="true" />
        规划中功能
        <Badge variant="outline">{{ plannedModules.length }}</Badge>
      </CardTitle>
    </CardHeader>
    <CardContent class="flex flex-col gap-2 text-sm text-muted-foreground">
      <p>以下模块尚在设计中，可点击查看各自的目标说明。</p>
      <ul class="flex flex-wrap gap-2">
        <li v-for="module in plannedModules" :key="module.id">
          <RouterLink
            :to="module.route.path"
            class="inline-flex items-center gap-1.5 rounded-md border border-dashed px-2 py-1 text-xs transition-colors hover:bg-accent hover:text-foreground"
          >
            <component :is="module.icon" class="size-3.5" aria-hidden="true" />
            {{ module.title }}
          </RouterLink>
        </li>
      </ul>
    </CardContent>
  </Card>
</template>
