/**
 * preload:通过 contextBridge 向渲染层暴露安全 API
 */
import { contextBridge, ipcRenderer } from 'electron'

const api = {
  /** 拖拽文件分流 */
  classifyFiles: (paths: string[]) => ipcRenderer.invoke('file:classify', paths),
  /** 看图 */
  listImagesInDir: (dir: string) => ipcRenderer.invoke('viewer:listDir', dir),
  readImage: (file: string) => ipcRenderer.invoke('viewer:read', file),
  /** 归档 */
  extract: (archivePath: string, destDir: string, password?: string) =>
    ipcRenderer.invoke('archive:extract', archivePath, destDir, password),
  compress: (req: unknown) => ipcRenderer.invoke('archive:compress', req),
  /** 任务 */
  cancelTask: (taskId: string) => ipcRenderer.invoke('task:cancel', taskId),
  listTasks: () => ipcRenderer.invoke('task:list'),
  /** 对话框/外壳 */
  pickPaths: () => ipcRenderer.invoke('dialog:pickDirs'),
  showInFolder: (p: string) => ipcRenderer.invoke('shell:showInFolder', p),
  /** 订阅任务事件 */
  onTaskCreated: (cb: (t: unknown) => void) => {
    const fn = (_e: unknown, t: unknown) => cb(t)
    ipcRenderer.on('task:created', fn)
    return () => ipcRenderer.removeListener('task:created', fn)
  },
  onTaskProgress: (cb: (p: { taskId: string; percent: number }) => void) => {
    const fn = (_e: unknown, p: unknown) => cb(p as never)
    ipcRenderer.on('task:progress', fn)
    return () => ipcRenderer.removeListener('task:progress', fn)
  },
  onTaskFinished: (cb: (t: unknown) => void) => {
    const fn = (_e: unknown, t: unknown) => cb(t)
    ipcRenderer.on('task:finished', fn)
    return () => ipcRenderer.removeListener('task:finished', fn)
  }
}

contextBridge.exposeInMainWorld('api', api)
