<script setup lang="ts">
import { onMounted, ref } from 'vue'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { toCommandError } from '@/api/errors'
import { useKimiStore } from '@/stores/kimi'
import { useSettingsStore, type ThemeMode } from '@/stores/settings'

const kimi = useKimiStore()
const settings = useSettingsStore()

// Kimi 开放平台表单；key 永不回显，只显示「已配置/未配置」
const accountName = ref('')
const apiBase = ref('')
const apiKey = ref('')
const saving = ref(false)
const saveMessage = ref('')

const themeOptions: { value: ThemeMode; label: string }[] = [
  { value: 'light', label: '亮色' },
  { value: 'dark', label: '暗色' },
  { value: 'system', label: '跟随系统' },
]

onMounted(async () => {
  await kimi.loadAccount()
  if (kimi.account) {
    accountName.value = kimi.account.name
    apiBase.value = kimi.account.api_base
  }
})

async function saveAccount() {
  saving.value = true
  saveMessage.value = ''
  try {
    // apiKey 留空表示沿用 keyring 中已有 key（见 kimi_account_set 约定）
    await kimi.saveAccount(
      {
        name: accountName.value || '默认账户',
        apiBase: apiBase.value,
        enabled: true,
      },
      apiKey.value || undefined,
    )
    apiKey.value = ''
    saveMessage.value = '已保存（API Key 已写入系统钥匙串）'
  } catch (err) {
    saveMessage.value = `保存失败：${toCommandError(err).message}`
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold tracking-tight">设置</h1>
      <p class="mt-1 text-sm text-muted-foreground">各模块配置入口（S1-2 基础版）。</p>
    </header>

    <!-- M6-01：Kimi 开放平台 -->
    <Card>
      <CardHeader>
        <div class="flex items-center gap-2">
          <CardTitle class="text-base">Kimi 开放平台</CardTitle>
          <Badge :variant="kimi.account?.has_credential ? 'default' : 'secondary'">
            {{ kimi.account?.has_credential ? '已配置' : '未配置' }}
          </Badge>
        </div>
        <CardDescription>
          用于查询开放平台余额。API Key 只写入系统钥匙串，界面不回显。
        </CardDescription>
      </CardHeader>
      <CardContent class="flex max-w-lg flex-col gap-4">
        <div class="flex flex-col gap-1.5">
          <Label for="account-name">账户名称</Label>
          <Input id="account-name" v-model="accountName" placeholder="默认账户" />
        </div>
        <div class="flex flex-col gap-1.5">
          <Label for="api-base">API 地址</Label>
          <Input id="api-base" v-model="apiBase" placeholder="https://api.moonshot.cn" />
        </div>
        <div class="flex flex-col gap-1.5">
          <Label for="api-key">API Key</Label>
          <Input
            id="api-key"
            v-model="apiKey"
            type="password"
            :placeholder="kimi.account?.has_credential ? '已保存，留空则不修改' : 'sk-...'"
          />
        </div>
        <div class="flex items-center gap-3">
          <Button :disabled="saving" @click="saveAccount">
            {{ saving ? '保存中…' : '保存' }}
          </Button>
          <span v-if="saveMessage" class="text-sm text-muted-foreground">{{ saveMessage }}</span>
        </div>
      </CardContent>
    </Card>

    <!-- M6-05：外观 -->
    <Card>
      <CardHeader>
        <CardTitle class="text-base">外观</CardTitle>
        <CardDescription>主题偏好保存在本机，立即生效。</CardDescription>
      </CardHeader>
      <CardContent class="max-w-lg">
        <div class="flex flex-col gap-1.5">
          <Label>主题</Label>
          <Select v-model="settings.theme">
            <SelectTrigger class="w-48">
              <SelectValue placeholder="选择主题" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="opt in themeOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </CardContent>
    </Card>

    <!-- Kimi 日志目录覆盖 -->
    <Card>
      <CardHeader>
        <CardTitle class="text-base">Kimi 日志目录</CardTitle>
        <CardDescription
          >留空 = 自动发现（~/.kimi-code/sessions），修改后手动重新扫描生效。</CardDescription
        >
      </CardHeader>
      <CardContent class="max-w-lg">
        <Input v-model="settings.kimiSessionsDir" placeholder="留空使用默认目录" />
      </CardContent>
    </Card>

    <Separator />
    <p class="text-xs text-muted-foreground">
      数据源模式：{{ settings.datasourceMode }}（S2-2 可切换）
    </p>
  </div>
</template>
