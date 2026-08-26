import type { GalleryPhoto } from '~~/shared/types/photo'

export type PhotoExportFormat = 'webp' | 'png' | 'jpg'

interface PhotoTransferState {
  active: boolean
  label: string
  completed: number
  total: number
}

const formatExtension = (format: PhotoExportFormat) => format === 'jpg' ? 'jpg' : format
const formatMime = (format: PhotoExportFormat) => format === 'jpg' ? 'image/jpeg' : `image/${format}`

const safeDownloadName = (photo: GalleryPhoto, format: PhotoExportFormat) => {
  const base = (photo.title || `photo-${photo.id}`)
    .split(/[\\/]/).at(-1)!
    .replace(/\.[^.]+$/, '')
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_')
    .trim() || `photo-${photo.id}`
  return `${base}.${formatExtension(format)}`
}

const responseBlob = async (response: Response) => {
  if (response.ok) return response.blob()
  let message = `请求失败（${response.status}）`
  try {
    const payload = await response.json() as { error?: string }
    if (payload.error) message = payload.error
  } catch {
    // The fallback status message is already useful.
  }
  throw new Error(message)
}

const saveBlob = (blob: Blob, filename: string) => {
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = filename
  anchor.style.display = 'none'
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  window.setTimeout(() => URL.revokeObjectURL(url), 30_000)
}

export function usePhotoActions() {
  const toast = useToast()
  const transfer = useState<PhotoTransferState>('photo-transfer-state', () => ({
    active: false,
    label: '',
    completed: 0,
    total: 0,
  }))

  const renderUrl = (photo: GalleryPhoto, format: PhotoExportFormat, download = false) => {
    const query = new URLSearchParams({ format })
    if (download) query.set('download', 'true')
    return `${photo.renderUrl}?${query}`
  }

  const downloadOne = async (photo: GalleryPhoto, format: PhotoExportFormat, quiet = false) => {
    const blob = await responseBlob(await fetch(renderUrl(photo, format, true), {
      credentials: 'same-origin',
    }))
    saveBlob(blob, safeDownloadName(photo, format))
    if (!quiet) {
      toast.add({
        title: '下载已开始',
        description: `${safeDownloadName(photo, format)} · 由高清 WebP 版本导出`,
        color: 'success',
      })
    }
  }

  const copyOne = async (photo: GalleryPhoto, format: PhotoExportFormat) => {
    if (!window.isSecureContext || !navigator.clipboard?.write || typeof ClipboardItem === 'undefined') {
      toast.add({
        title: '当前地址不能复制图片',
        description: '浏览器只允许 HTTPS 页面写入图片剪贴板；请使用 HTTPS 访问，或改用下载。',
        color: 'warning',
      })
      return
    }
    transfer.value = { active: true, label: `正在复制为 ${format.toUpperCase()}`, completed: 0, total: 1 }
    try {
      const blob = await responseBlob(await fetch(renderUrl(photo, format), {
        credentials: 'same-origin',
      }))
      const mime = formatMime(format)
      const clipboardType = blob.type || mime
      const clipboard = new ClipboardItem({ [clipboardType]: blob })
      await navigator.clipboard.write([clipboard])
      transfer.value.completed = 1
      toast.add({
        title: `已复制为 ${format.toUpperCase()}`,
        description: '图片已经写入系统剪贴板。',
        color: 'success',
      })
    } catch (error) {
      toast.add({
        title: '复制失败',
        description: error instanceof Error
          ? `${error.message}。部分浏览器只允许复制 PNG 图片。`
          : '浏览器不支持这种剪贴板图片格式。',
        color: 'error',
      })
    } finally {
      transfer.value.active = false
    }
  }

  const downloadMany = async (
    photos: GalleryPhoto[],
    format: PhotoExportFormat,
    mobile: boolean,
  ) => {
    if (!photos.length || transfer.value.active) return
    transfer.value = {
      active: true,
      label: mobile ? `正在逐张下载 ${format.toUpperCase()}` : '正在生成 ZIP',
      completed: 0,
      total: photos.length,
    }
    try {
      if (mobile) {
        for (const photo of photos) {
          await downloadOne(photo, format, true)
          transfer.value.completed += 1
          await new Promise(resolve => window.setTimeout(resolve, 180))
        }
        toast.add({
          title: `已开始下载 ${photos.length} 张图片`,
          description: '移动端按顺序逐张保存；浏览器可能会询问是否允许多个下载。',
          color: 'success',
        })
      } else {
        const blob = await responseBlob(await fetch('/api/photos/export', {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ photoIds: photos.map(photo => photo.id), format }),
        }))
        transfer.value.completed = photos.length
        saveBlob(blob, `chronoframe-${photos.length}-${formatExtension(format)}.zip`)
        toast.add({
          title: 'ZIP 已生成',
          description: `${photos.length} 张 ${format.toUpperCase()} 图片开始下载。`,
          color: 'success',
        })
      }
    } catch (error) {
      toast.add({
        title: mobile ? '批量下载中断' : 'ZIP 生成失败',
        description: error instanceof Error ? error.message : '请稍后重试。',
        color: 'error',
      })
    } finally {
      transfer.value.active = false
    }
  }

  return { transfer, renderUrl, downloadOne, downloadMany, copyOne }
}
