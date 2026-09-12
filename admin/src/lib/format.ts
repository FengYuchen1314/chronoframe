// 格式化与状态映射。移植自 Vue 侧的 app/utils/adminFormat.ts 与 settings/storage.vue。

/**
 * 1000 进制字节格式化。用于上传队列、下载包体积、相册图片大小。
 * 注意：与 formatBytes（1024 进制）是两个不同的函数，不要合并——
 * 现网不同位置显示不同，合并会改变既有文案。
 */
export function adminBytes(bytes: number): string {
  if (!bytes || bytes < 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const index = Math.min(4, Math.floor(Math.log(bytes) / Math.log(1000)))
  return `${(bytes / 1000 ** index).toFixed(index ? 1 : 0)} ${units[index]}`
}

/** 1024 进制。用于存储设置页的 S3 空间统计（与后端常量口径一致）。 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), 4)
  const value = bytes / 1024 ** unit
  return `${value >= 100 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`
}

/** Unix 秒 -> 本地时间字符串（保持现网 zh-CN + 24 小时制的呈现）。 */
export function formatDateTime(seconds: number): string {
  if (!seconds) return '—'
  return new Date(seconds * 1000).toLocaleString('zh-CN', { hour12: false })
}

/** 只有时分秒，用于「连接测试通过」提示。 */
export function formatTime(date: Date): string {
  return date.toLocaleTimeString('zh-CN', { hour12: false })
}

export type ChipTone = 'default' | 'accent' | 'success' | 'warning' | 'danger'

/* ----------------------------- 下载任务状态 ----------------------------- */

export const downloadStatusText: Record<string, string> = {
  queued: '排队中',
  running: '正在打包',
  ready: '可下载',
  failed: '生成失败',
  cancelled: '已取消',
  deleting: '正在删除',
  deleted: '已删除',
}

export const downloadStatusTone: Record<string, ChipTone> = {
  queued: 'default',
  running: 'accent',
  ready: 'success',
  failed: 'danger',
  cancelled: 'warning',
  deleting: 'accent',
  deleted: 'default',
}

/* ----------------------------- 上传队列状态 ----------------------------- */

export const uploadStatusText: Record<string, string> = {
  queued: '等待上传',
  uploading: '上传中',
  // 注意：现网文案是「未确认成功」而不是「失败」——丢了响应的请求无法判断
  // 服务端是否已入库，重试可能产生重复，这个措辞是在提示这一点。
  failed: '未确认成功',
  done: '已入库',
}

export const uploadStatusTone: Record<string, ChipTone> = {
  queued: 'default',
  uploading: 'accent',
  failed: 'danger',
  done: 'success',
}

/* ------------------------------ 存储迁移状态 ----------------------------- */

export function migrationStatusText(job: StorageMigrationJobLike): string {
  if (job.cleanupStatus === 'cleaning') return '正在清理旧存储'
  if (job.status === 'completed') {
    const cleanup: Record<string, string> = {
      not_ready: '迁移完成',
      pending: '等待处理旧存储',
      cleaning: '正在清理旧存储',
      cleaned: '旧存储已清理',
      retained: '旧存储已保留',
      failed: '旧存储清理失败',
      interrupted: '旧存储清理已中断',
    }
    return cleanup[job.cleanupStatus] ?? '迁移完成'
  }
  const status: Record<string, string> = {
    queued: '等待开始',
    running: '正在迁移',
    failed: '迁移失败',
    cancelled: '迁移已中断',
    interrupted: '迁移被重启中断',
  }
  return status[job.status] ?? job.status
}

export function migrationStatusTone(job: StorageMigrationJobLike): ChipTone {
  if (job.cleanupStatus === 'cleaned' || job.cleanupStatus === 'retained') return 'success'
  if (job.status === 'failed' || job.cleanupStatus === 'failed') return 'danger'
  if (
    job.status === 'cancelled' || job.status === 'interrupted' ||
    job.cleanupStatus === 'interrupted'
  ) return 'warning'
  return 'accent'
}

interface StorageMigrationJobLike {
  status: string
  cleanupStatus: string
}

/* ------------------------------ 缓存重建状态 ----------------------------- */

export function thumbnailStatusText(job: ThumbnailJobLike | null): string {
  if (!job) return '尚未手动重建'
  if (job.status === 'running') return job.phase === 'clearing' ? '正在清空缓存' : '正在并发生成'
  const status: Record<string, string> = {
    queued: '等待开始',
    completed: '重建完成',
    failed: '部分生成失败',
    cancelled: '已安全中断',
    interrupted: '服务重启后待恢复',
  }
  return status[job.status] ?? job.status
}

export function thumbnailStatusTone(job: ThumbnailJobLike | null): ChipTone {
  if (!job) return 'default'
  if (job.status === 'completed') return 'success'
  if (job.status === 'failed') return 'danger'
  if (job.status === 'cancelled' || job.status === 'interrupted') return 'warning'
  return 'accent'
}

interface ThumbnailJobLike {
  status: string
  phase: string
}

/* ----------------------------- S3 清理状态 ------------------------------ */

export function s3CleanupStatusText(job: S3CleanupJobLike | null): string {
  if (!job) return '尚未扫描'
  if (job.status === 'running') return job.phase === 'scanning' ? '正在扫描对象' : '正在并发清理'
  if (job.status === 'ready') return job.total ? '等待确认清理' : '空间干净'
  const status: Record<string, string> = {
    completed: '清理完成',
    failed: '任务失败',
    cancelled: '已安全中断',
    interrupted: '服务重启后待继续',
  }
  return status[job.status] ?? job.status
}

export function s3CleanupStatusTone(job: S3CleanupJobLike | null): ChipTone {
  if (!job) return 'default'
  if (job.status === 'completed' || (job.status === 'ready' && job.total === 0)) return 'success'
  if (job.status === 'failed') return 'danger'
  if (job.status === 'cancelled' || job.status === 'interrupted' || job.status === 'ready') {
    return 'warning'
  }
  return 'accent'
}

interface S3CleanupJobLike {
  status: string
  phase: string
  total: number
}

/**
 * 百分比收敛到 [0,100]，避免后端计数瞬时超过总量时进度条溢出。
 * `total === 0` 一律给 0 —— 存储迁移（migrationProgress）就是这个语义。
 */
export function progressPercent(completed: number, total: number): number {
  if (!total) return 0
  return Math.min(100, Math.round((completed / total) * 100))
}

/**
 * 任务进度百分比，`total === 0` 时按状态兜底。
 *
 * 与 progressPercent 的唯一区别就在这个兜底上：缓存重建与 S3 清理可能
 * 「没有任何待处理项就直接完成」（库里没图、或候选对象为 0）。现网
 * thumbnailProgress / s3CleanupProgress 在这种情况下显示 100%，否则状态标签
 * 写着「重建完成 / 清理完成」而进度条停在 0%，读起来自相矛盾。
 * 存储迁移**不**走这个兜底，保持 progressPercent。
 */
export function jobProgressPercent(
  completed: number,
  total: number,
  status: string | undefined,
): number {
  if (!total) return status === 'completed' ? 100 : 0
  return Math.min(100, Math.round((completed / total) * 100))
}
