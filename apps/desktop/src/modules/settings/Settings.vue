<script setup lang="ts">
import { onMounted } from 'vue'
import type { ThemeMode } from '@toogl/core-types'

import LoadErrorBanner from '@/shared/components/LoadErrorBanner.vue'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useSettingsStore } from './store'
import { version } from '../../../package.json'

const settings = useSettingsStore()
const themes: { value: ThemeMode; label: string }[] = [
  { value: 'light', label: '亮色' },
  { value: 'dark', label: '暗色' },
  { value: 'system', label: '跟随系统' },
]
onMounted(() => settings.init())
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold tracking-tight">设置</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        软件外观与基本信息，各工具的配置在对应模块中管理。
      </p>
    </header>
    <LoadErrorBanner v-if="settings.error" :error="settings.error" @retry="settings.init()" />
    <Card>
      <CardHeader>
        <CardTitle class="text-base">外观</CardTitle>
        <CardDescription>主题保存在本机，修改后立即生效。</CardDescription>
      </CardHeader>
      <CardContent class="flex flex-col gap-1.5">
        <Label>主题</Label>
        <Select
          :model-value="settings.theme"
          :disabled="settings.saving"
          @update:model-value="settings.setTheme($event as ThemeMode)"
        >
          <SelectTrigger class="w-48"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem v-for="theme in themes" :key="theme.value" :value="theme.value">{{
              theme.label
            }}</SelectItem>
          </SelectContent>
        </Select>
      </CardContent>
    </Card>
    <Card>
      <CardHeader><CardTitle class="text-base">关于 Toogl</CardTitle></CardHeader>
      <CardContent class="text-sm text-muted-foreground"
        >图格 · 个人 Windows 工具箱 · v{{ version }}</CardContent
      >
    </Card>
  </div>
</template>
