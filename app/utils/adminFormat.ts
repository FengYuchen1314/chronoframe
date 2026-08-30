export function adminBytes(bytes: number) {
  if (!bytes || bytes < 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const index = Math.min(4, Math.floor(Math.log(bytes) / Math.log(1000)))
  return `${(bytes / 1000 ** index).toFixed(index ? 1 : 0)} ${units[index]}`
}
export const downloadStatus: Record<string, string> = { queued: '排队中', running: '正在打包', ready: '可下载', failed: '生成失败', cancelled: '已取消', deleting: '正在删除', deleted: '已删除' }
export const downloadStatusColor: Record<string, string> = { queued: 'default', running: 'processing', ready: 'success', failed: 'error', cancelled: 'warning', deleting: 'processing', deleted: 'default' }
