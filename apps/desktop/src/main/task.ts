/**
 * 任务队列与进度广播
 * 7z 本身多线程,这里对归档任务串行排队;每个任务可取消,进度实时推给渲染层
 */
import { ChildProcess } from 'child_process'
import { BrowserWindow } from 'electron'

export type TaskStatus = 'running' | 'done' | 'error' | 'canceled' | 'NEED_PASSWORD'

export interface ArchiveTask {
  id: string
  title: string
  status: TaskStatus
  percent: number
  error?: string
  proc?: ChildProcess
  cancelRequested?: boolean
}

export interface TaskProgress {
  taskId: string
  percent: number
}

const tasks = new Map<string, ArchiveTask>()
let seq = 0

function broadcast(event: string, payload: unknown): void {
  for (const win of BrowserWindow.getAllWindows()) {
    win.webContents.send(event, payload)
  }
}

export function createTask(title: string): string {
  const id = `task-${++seq}`
  tasks.set(id, { id, title, status: 'running', percent: 0 })
  broadcast('task:created', tasks.get(id))
  return id
}

/** 更新进度或附加子进程/取消标记 */
export function updateTask(taskId: string, patch: Partial<ArchiveTask> | TaskProgress): void {
  const t = tasks.get(taskId)
  if (!t) return
  if ('percent' in patch && !('status' in patch)) {
    t.percent = (patch as TaskProgress).percent
    broadcast('task:progress', { taskId, percent: t.percent })
    return
  }
  Object.assign(t, patch)
  // 挂载子进程后检查取消请求
  if (t.cancelRequested && t.proc) {
    t.proc.kill()
  }
}

/** 结束任务并广播终态 */
export function finishTask(taskId: string, status: TaskStatus, error?: string): void {
  const t = tasks.get(taskId)
  if (!t) return
  t.status = status
  t.error = error
  if (status === 'done') t.percent = 100
  broadcast('task:finished', t)
}

/** 列出当前任务快照(渲染层刷新用) */
export function listTasks(): ArchiveTask[] {
  return [...tasks.values()]
}
