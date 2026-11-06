/**
 * 归档引擎封装:统一通过内嵌的 7z.exe 完成压缩/解压/列表
 * 7-Zip 以独立 exe 分发(LGPL,未修改、未静态链接),本模块只做进程调用
 */
import { spawn, ChildProcess } from 'child_process'
import { existsSync } from 'fs'
import path from 'path'
import { app } from 'electron'
import { createTask, updateTask, finishTask, TaskProgress } from './task'

/** 7z.exe 的候选路径:开发环境在 resources/bin,打包后随 asar.unpacked */
export function resolve7zPath(): string {
  const candidates = [
    path.join(process.resourcesPath ?? '', 'bin', '7z.exe'),
    path.join(__dirname, '../../resources/bin/7z.exe'),
    path.join(app.getAppPath(), 'resources/bin/7z.exe')
  ]
  for (const p of candidates) {
    if (p && existsSync(p)) return p
  }
  return '7z' // 兜底:依赖系统 PATH 中的 7z
}

/** 解析 7z -bsp1 输出行中的百分比数字 */
function parsePercent(chunk: string): number | null {
  const m = chunk.match(/(\d{1,3})\s*%/)
  if (!m) return null
  const v = parseInt(m[1], 10)
  return Number.isNaN(v) ? null : v
}

/** 常见压缩格式扩展名判断(供拖拽分流);静态字面量用 Record 查表 */
const ARCHIVE_EXT_BY_NAME: Record<string, true> = {
  zip: true, '7z': true, rar: true, tar: true, gz: true, tgz: true,
  bz2: true, xz: true, zst: true, iso: true, cab: true
}

export function isArchiveFile(filePath: string): boolean {
  const ext = path.extname(filePath).slice(1).toLowerCase()
  return ARCHIVE_EXT_BY_NAME[ext] === true
}

interface RunOptions {
  /** 任务标题,用于进度展示 */
  title: string
  /** 需要密码时传入;null 表示首次尝试(失败后会返回 NEED_PASSWORD) */
  password?: string | null
}

/** 运行 7z 并跟踪进度;返回任务 id,密码错误时任务以 NEED_PASSWORD 状态结束 */
function run7z(args: string[], opts: RunOptions): string {
  const taskId = createTask(opts.title)
  const proc: ChildProcess = spawn(resolve7zPath(), args, { windowsHide: true })
  // 将子进程挂在任务上,便于取消
  updateTask(taskId, { proc })

  let stderrBuf = ''
  proc.stdout?.on('data', (d: Buffer) => {
    const pct = parsePercent(d.toString())
    if (pct !== null) {
      const prog: TaskProgress = { taskId, percent: pct }
      updateTask(taskId, prog)
    }
  })
  proc.stderr?.on('data', (d: Buffer) => { stderrBuf += d.toString() })

  proc.on('close', (code) => {
    if (code === 0) {
      finishTask(taskId, 'done')
    } else if (stderrBuf.includes('Wrong password') || stderrBuf.includes('Enter password')) {
      finishTask(taskId, 'NEED_PASSWORD', '需要密码')
    } else {
      finishTask(taskId, 'error', stderrBuf.trim() || `7z 退出码 ${code}`)
    }
  })
  return taskId
}

/** 解压压缩包到目标目录 */
export function extractArchive(archivePath: string, destDir: string, password?: string | null): string {
  const args = ['x', archivePath, `-o${destDir}`, '-y', '-bsp1']
  if (password) args.push(`-p${password}`)
  else args.push('-p') // 空密码占位,避免交互卡住
  return run7z(args, { title: `解压 ${path.basename(archivePath)}` })
}

/** 压缩文件/目录到目标压缩包 */
export interface CompressRequest {
  targetPath: string
  sources: string[]
  /** 7z / zip / tar.gz;7z 才支持文件名加密 */
  format: '7z' | 'zip' | 'tar.gz'
  level?: number // 0-9,默认 5
  password?: string
  encryptNames?: boolean
}

export function compressFiles(req: CompressRequest): string {
  const args = ['a', req.targetPath, '-y', '-bsp1', `-mx${req.level ?? 5}`, '-mmt=on']
  if (req.format === '7z') args.push('-t7z')
  else if (req.format === 'zip') args.push('-tzip')
  else args.push('-ttar')
  if (req.password) {
    args.push(`-p${req.password}`)
    if (req.encryptNames && req.format === '7z') args.push('-mhe=on')
  }
  args.push(...req.sources)
  return run7z(args, { title: `压缩为 ${path.basename(req.targetPath)}` })
}

/** 取消任务:kill 子进程 */
export function cancelTask(taskId: string): void {
  updateTask(taskId, { cancelRequested: true })
}
