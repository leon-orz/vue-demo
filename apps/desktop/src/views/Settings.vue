<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'

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
import { useSettingsStore, type ThemeMode } from '@/stores/settings'
import { useUsageStore } from '@/stores/usage'

const usage = useUsageStore()
const settings = useSettingsStore()

/**
 * Kimi 账户表单状态；
 * key 永不回显，只显示「已配置/未配置」，留空保存表示沿用 keyring 已有 key
 */
const kimiForm = reactive({ name: '', apiBase: '', apiKey: '' })
const kimiSaving = ref(false)
const kimiMessage = ref('')

const themeOptions: { value: ThemeMode; label: string }[] = [
  { value: 'light', label: '亮色' },
  { value: 'dark', label: '暗色' },
  { value: 'system', label: '跟随系统' },
]

onMounted(async () => {
  await usage.loadAccounts()
  const account = usage.accounts.kimi
  if (account) {
    kimiForm.name = account.name
    kimiForm.apiBase = account.api_base ?? ''
  }
})

async function saveKimiAccount() {
  kimiSaving.value = true
  kimiMessage.value = ''
  try {
    await usage.saveAccount(
      'kimi',
      {
        platform: 'kimi',
        name: kimiForm.name || '默认账户',
        api_base: kimiForm.apiBase || null,
        quota_total: null,
        quota_cycle: null,
        enabled: true,
      },
      // apiKey 留空表示沿用 keyring 中已有 key（见 platform_account_set 约定）
      kimiForm.apiKey || undefined,
    )
    kimiForm.apiKey = ''
    kimiMessage.value = '已保存（API Key 已写入系统钥匙串）'
  } catch (err) {
    kimiMessage.value = `保存失败：${toCommandError(err).message}`
  } finally {
    kimiSaving.value = false
  }
}

/** 「已配置」标记（徽章用）：看 keyring 凭据 */
const kimiConfigured = computed(() => usage.accounts.kimi?.has_credential === true)
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold tracking-tight">设置</h1>
      <p class="mt-1 text-sm text-muted-foreground">AI 用量平台与通用偏好配置。</p>
    </header>

    <!-- M6-01：AI 用量平台账户（Kimi；其他平台已于 2026-10-02 砍掉，回归时从 git 历史恢复） -->
    <Card>
      <CardHeader>
        <div class="flex items-center gap-2">
          <CardTitle class="text-base">Kimi 开放平台</CardTitle>
          <Badge :variant="kimiConfigured ? 'default' : 'secondary'">
            {{ kimiConfigured ? '已配置' : '未配置' }}
          </Badge>
        </div>
        <CardDescription>
          用于查询开放平台余额与套餐用量。API Key 只写入系统钥匙串，界面不回显。
        </CardDescription>
      </CardHeader>
      <CardContent class="flex max-w-lg flex-col gap-4">
        <div class="flex flex-col gap-1.5">
          <Label for="kimi-account-name">账户名称</Label>
          <Input id="kimi-account-name" v-model="kimiForm.name" placeholder="默认账户" />
        </div>
        <div class="flex flex-col gap-1.5">
          <Label for="kimi-api-base">API 地址</Label>
          <Input
            id="kimi-api-base"
            v-model="kimiForm.apiBase"
            placeholder="https://api.moonshot.cn"
          />
        </div>
        <div class="flex flex-col gap-1.5">
          <Label for="kimi-api-key">API Key</Label>
          <Input
            id="kimi-api-key"
            v-model="kimiForm.apiKey"
            type="password"
            :placeholder="usage.accounts.kimi?.has_credential ? '已保存，留空则不修改' : 'sk-...'"
          />
        </div>
        <div class="flex items-center gap-3">
          <Button :disabled="kimiSaving" @click="saveKimiAccount">
            {{ kimiSaving ? '保存中…' : '保存' }}
          </Button>
          <span v-if="kimiMessage" class="text-sm text-muted-foreground">{{ kimiMessage }}</span>
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
        <CardTitle class="text-base">日志目录覆盖</CardTitle>
        <CardDescription>
          Kimi：留空 = 自动发现（~/.kimi-code/sessions），修改后手动重新扫描生效。
        </CardDescription>
      </CardHeader>
      <CardContent class="flex max-w-lg flex-col gap-1.5">
        <Label for="kimi-sessions-dir">Kimi 日志目录</Label>
        <Input
          id="kimi-sessions-dir"
          v-model="settings.kimiSessionsDir"
          placeholder="留空使用默认目录"
        />
      </CardContent>
    </Card>

    <Separator />
    <p class="text-xs text-muted-foreground">
      数据源模式：{{ settings.datasourceMode }}（S2-2 可切换）
    </p>
  </div>
</template>
