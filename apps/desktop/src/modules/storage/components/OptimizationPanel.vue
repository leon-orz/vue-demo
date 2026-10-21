<script setup lang="ts">
import { Activity } from '@lucide/vue'
import { computed } from 'vue'

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'

const props = defineProps<{ systemDriveUsage: number | null }>()
const remainingPercent = computed(() =>
  props.systemDriveUsage === null ? null : Math.round(props.systemDriveUsage * 100),
)
</script>

<template>
  <section class="grid grid-cols-1 gap-4 lg:grid-cols-2">
    <Card>
      <CardHeader>
        <CardTitle class="flex items-center gap-2 text-base">
          <Activity class="size-4" />系统优化
        </CardTitle>
        <CardDescription>第一版提供检查方向，不修改系统设置。</CardDescription>
      </CardHeader>
      <CardContent class="space-y-4 text-sm">
        <div>
          <h2 class="font-medium">启动项</h2>
          <p class="mt-1 text-muted-foreground">可在 Windows“设置 → 应用 → 启动”检查不常用应用。</p>
        </div>
        <div>
          <h2 class="font-medium">存储空间</h2>
          <p v-if="remainingPercent !== null" class="mt-1 text-muted-foreground">
            系统盘剩余 {{ remainingPercent }}%；
            {{
              remainingPercent < 15
                ? '剩余空间偏低，可检查 Windows 存储建议。'
                : '当前剩余空间充足。'
            }}
          </p>
          <p v-else class="mt-1 text-muted-foreground">
            查看系统“存储”页面中的临时文件和存储感知建议。
          </p>
        </div>
        <div>
          <h2 class="font-medium">后台应用</h2>
          <p class="mt-1 text-muted-foreground">
            检查不需要常驻的应用自身设置；不要关闭不熟悉的系统服务。
          </p>
        </div>
      </CardContent>
    </Card>
    <Card>
      <CardHeader><CardTitle class="text-base">安全边界</CardTitle></CardHeader>
      <CardContent class="text-sm text-muted-foreground">
        本页不自动清理注册表、不关闭系统服务，也不更改启动项、电源计划或网络配置。
      </CardContent>
    </Card>
  </section>
</template>
