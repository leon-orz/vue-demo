/**
 * 看图辅助:目录内图片列表浏览、EXIF 提取、缩略图
 * 常规格式(jpg/png/webp/gif/svg/bmp/ico)由 Chromium 原生解码,主进程只做文件 IO
 */
import { readdir, readFile } from 'fs/promises'
import path from 'path'

/** Chromium 原生支持的图片扩展名 */
const IMAGE_EXT_BY_NAME: Record<string, true> = {
  jpg: true, jpeg: true, png: true, gif: true, webp: true,
  svg: true, bmp: true, ico: true, avif: true
}

export function isImageFile(filePath: string): boolean {
  const ext = path.extname(filePath).slice(1).toLowerCase()
  return IMAGE_EXT_BY_NAME[ext] === true
}

/** 列出目录内全部图片文件(按名称排序,供翻页) */
export async function listImagesInDir(dir: string): Promise<string[]> {
  const entries = await readdir(dir, { withFileTypes: true })
  return entries
    .filter((e) => e.isFile() && isImageFile(e.name))
    .map((e) => path.join(dir, e.name))
    .sort((a, b) => a.localeCompare(b, 'zh-Hans-CN', { numeric: true }))
}

/** 读取图片为 Buffer(渲染层转 blob URL 显示) */
export async function readImageFile(filePath: string): Promise<Buffer> {
  return readFile(filePath)
}

/** EXIF 提取在渲染层直接用 exifr 解析同一 Buffer,主进程无需重复实现 */
