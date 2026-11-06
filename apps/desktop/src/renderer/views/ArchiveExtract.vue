<script setup lang="ts">
/**
 * 解压页:选择压缩包 → 目标目录 → 密码 → 进度
 */
import { ref, watch } from 'vue'
import { useRoute } from 'vue-router'

const route = useRoute()

const archivePath = ref('')
const destDir = ref('')
const password = ref('')
const taskId = ref('')
const percent = ref(0)
const status = ref<'idle' | 'running' | 'done' | 'error' | 'NEED_PASSWORD' | 'canceled'>('idle')
const errorMsg = ref('')

watch(() => route.query.file, (f) => {
  if (typeof f === 'string' && f) {
    archivePath.value = f
    // 默认解压到压缩包同名目录
    destDir.value = f.replace(/\.[^.\\/]+$/, '')
    status.value = 'idle'
  }
}, { immediate: true })

async function pickArchive(): Promise<void> {
  const file = await window.api.pickArchive()
  if (file) {
    archivePath.value = file
    destDir.value = file.replace(/\.[^.\\/]+$/, '')
    status.value = 'idle'
  }
}

function startExtract(): void {
  if (!archivePath.value || !destDir.value) return
  percent.value = 0
  status.value = 'running'
  taskId.value = window.api.extract(archivePath.value, destDir.value, password.value || undefined)
}

function cancel(): void {
  if (taskId.value) window.api.cancelTask(taskId.value)
}

window.api.onTaskProgress((p) => {
  if (p.taskId === taskId.value) percent.value = p.percent
})

window.api.onTaskFinished((t: never) => {
  const task = t as { id: string; status: string; error?: string }
  if (task.id !== taskId.value) return
  if (task.status === 'done') status.value = 'done'
  else if (task.status === 'NEED_PASSWORD') {
    status.value = 'NEED_PASSWORD'
    errorMsg.value = '该压缩包需要密码'
  } else if (task.status === 'error') {
    status.value = 'error'
    errorMsg.value = task.error ?? '解压失败'
  } else if (task.status === 'canceled') status.value = 'canceled'
})

function openFolder(): void {
  window.api.showInFolder(destDir.value)
}
</script>

<template>
  <div class="page">
    <h2>解压</h2>
    <div class="form">
      <label>压缩包</label>
      <div class="row">
        <input v-model="archivePath" placeholder="拖入压缩包或点击右侧按钮选择" readonly />
        <button @click="pickArchive">选择…</button>
      </div>
      <label>解压到</label>
      <input v-model="destDir" placeholder="目标目录" />
      <label>密码(可选)</label>
      <input v-model="password" type="password" placeholder="加密压缩包的密码" />
      <div class="actions">
        <button class="primary" :disabled="status === 'running'" @click="startExtract">
          {{ status === 'running' ? '解压中…' : '开始解压' }}
        </button>
        <button v-if="status === 'running'" @click="cancel">取消</button>
        <button v-if="status === 'done'" @click="openFolder">打开所在文件夹</button>
      </div>
    </div>
    <div v-if="status === 'running' || status === 'done'" class="progress-row">
      <div class="progress"><div class="bar" :style="{ width: percent + '%' }" /></div>
      <span class="pct">{{ percent }}%</span>
    </div>
    <p v-if="status === 'NEED_PASSWORD'" class="msg warn">需要密码,请填写后重试</p>
    <p v-else-if="status === 'error'" class="msg err">{{ errorMsg }}</p>
    <p v-else-if="status === 'done'" class="msg ok">解压完成</p>
    <p v-else-if="status === 'canceled'" class="msg warn">已取消</p>
  </div>
</template>

<style scoped>
.page { padding: 24px 32px; overflow-y: auto; }
h2 { margin-bottom: 20px; font-size: 18px; color: #eee; }
.form { display: flex; flex-direction: column; gap: 8px; max-width: 640px; }
label { font-size: 13px; color: #999; margin-top: 8px; }
.row { display: flex; gap: 8px; }
input {
  flex: 1;
  background: #2a2c31;
  border: 1px solid #3a3d44;
  border-radius: 6px;
  padding: 8px 12px;
  color: #ddd;
  font-size: 13px;
  outline: none;
}
input:focus { border-color: #7ab8ff; }
button {
  background: #2a2c31;
  border: 1px solid #3a3d44;
  border-radius: 6px;
  padding: 8px 16px;
  color: #ddd;
  cursor: pointer;
  font-size: 13px;
}
button:hover { background: #33363c; }
button.primary { background: #2b5fad; border-color: #2b5fad; }
button.primary:disabled { opacity: 0.6; cursor: default; }
.actions { display: flex; gap: 10px; margin-top: 16px; }
.progress-row { display: flex; align-items: center; gap: 12px; margin-top: 20px; max-width: 640px; }
.progress { flex: 1; height: 8px; background: #2a2c31; border-radius: 4px; overflow: hidden; }
.bar { height: 100%; background: #7ab8ff; transition: width 0.2s; }
.pct { font-size: 13px; color: #999; min-width: 40px; text-align: right; }
.msg { margin-top: 16px; font-size: 14px; }
.ok { color: #6bd66b; }
.warn { color: #e6b455; }
.err { color: #e57272; }
</style>
