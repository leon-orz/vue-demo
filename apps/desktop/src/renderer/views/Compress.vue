<script setup lang="ts">
/**
 * 压缩页:选择来源 → 格式/密码 → 进度
 */
import { ref } from 'vue'

const sources = ref<string[]>([])
const targetPath = ref('')
const format = ref<'7z' | 'zip' | 'tar.gz'>('7z')
const level = ref(5)
const password = ref('')
const encryptNames = ref(true)

const taskId = ref('')
const percent = ref(0)
const status = ref<'idle' | 'running' | 'done' | 'error' | 'canceled'>('idle')
const errorMsg = ref('')

async function pickSources(): Promise<void> {
  // 压缩来源支持多选文件与文件夹:先目录,再补充单选;MVP 先支持文件夹选择
  const dir = await window.api.pickDir()
  if (dir) {
    sources.value = [dir]
    if (!targetPath.value) {
      targetPath.value = `${dir}/archive.${format.value === 'tar.gz' ? 'tar' : format.value}`
    }
  }
}

function startCompress(): void {
  if (sources.value.length === 0 || !targetPath.value) return
  percent.value = 0
  status.value = 'running'
  taskId.value = window.api.compress({
    targetPath: targetPath.value,
    sources: sources.value,
    format: format.value,
    level: level.value,
    password: password.value || undefined,
    encryptNames: encryptNames.value
  })
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
  else if (task.status === 'error') {
    status.value = 'error'
    errorMsg.value = task.error ?? '压缩失败'
  } else if (task.status === 'canceled') status.value = 'canceled'
})

function openFolder(): void {
  window.api.showInFolder(targetPath.value)
}
</script>

<template>
  <div class="page">
    <h2>压缩</h2>
    <div class="form">
      <label>来源(文件/文件夹)</label>
      <div class="row">
        <div class="sources">
          <span v-if="sources.length === 0" class="placeholder">尚未选择</span>
          <span v-for="s in sources" :key="s" class="chip">{{ s.split(/[\\/]/).pop() }}</span>
        </div>
        <button @click="pickSources">选择…</button>
      </div>
      <label>输出文件</label>
      <input v-model="targetPath" placeholder="输出压缩包完整路径" />
      <label>格式</label>
      <div class="row">
        <select v-model="format">
          <option value="7z">7z(推荐,支持加密文件名)</option>
          <option value="zip">zip(兼容性最好)</option>
          <option value="tar.gz">tar(打包,不压缩)</option>
        </select>
      </div>
      <label>压缩等级:{{ level }}</label>
      <input v-model.number="level" type="range" min="0" max="9" />
      <label>密码(可选)</label>
      <input v-model="password" type="password" placeholder="留空则不加密" />
      <label v-if="format === '7z' && password">
        <input v-model="encryptNames" type="checkbox" /> 同时加密文件名
      </label>
      <div class="actions">
        <button class="primary" :disabled="status === 'running'" @click="startCompress">
          {{ status === 'running' ? '压缩中…' : '开始压缩' }}
        </button>
        <button v-if="status === 'running'" @click="cancel">取消</button>
        <button v-if="status === 'done'" @click="openFolder">打开所在文件夹</button>
      </div>
    </div>
    <div v-if="status === 'running' || status === 'done'" class="progress-row">
      <div class="progress"><div class="bar" :style="{ width: percent + '%' }" /></div>
      <span class="pct">{{ percent }}%</span>
    </div>
    <p v-if="status === 'error'" class="msg err">{{ errorMsg }}</p>
    <p v-else-if="status === 'done'" class="msg ok">压缩完成</p>
    <p v-else-if="status === 'canceled'" class="msg warn">已取消</p>
  </div>
</template>

<style scoped>
.page { padding: 24px 32px; overflow-y: auto; }
h2 { margin-bottom: 20px; font-size: 18px; color: #eee; }
.form { display: flex; flex-direction: column; gap: 8px; max-width: 640px; }
label { font-size: 13px; color: #999; margin-top: 8px; }
.row { display: flex; gap: 8px; align-items: center; }
.sources {
  flex: 1;
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  min-height: 38px;
  background: #2a2c31;
  border: 1px solid #3a3d44;
  border-radius: 6px;
  padding: 6px 10px;
}
.placeholder { color: #666; font-size: 13px; align-self: center; }
.chip {
  background: #3a3d44;
  border-radius: 4px;
  padding: 2px 8px;
  font-size: 12px;
}
input, select {
  flex: 1;
  background: #2a2c31;
  border: 1px solid #3a3d44;
  border-radius: 6px;
  padding: 8px 12px;
  color: #ddd;
  font-size: 13px;
  outline: none;
}
input:focus, select:focus { border-color: #7ab8ff; }
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
