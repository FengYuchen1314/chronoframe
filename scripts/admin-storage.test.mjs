import test from 'node:test'
import assert from 'node:assert/strict'
// 存储设置页的纯逻辑：进度百分比与三组状态映射。
// 这些函数决定「状态标签」和「进度条」是否自相矛盾，逐条对齐重写前的 storage.vue。
import {
  formatBytes, jobProgressPercent, migrationStatusText, migrationStatusTone, progressPercent,
  s3CleanupStatusText, s3CleanupStatusTone, thumbnailStatusText, thumbnailStatusTone,
} from '../admin/src/lib/format.ts'

// 回归：total===0 时两种语义必须分开。
// 缓存重建与 S3 清理可能「没有待处理项就直接完成」（库里没图、候选对象为 0），
// 现网此时显示 100%；存储迁移没有这个兜底，显示 0%。
// 三处共用 progressPercent 会让「重建完成」配着 0% 的空进度条。
test('completed-with-nothing-to-do shows a full bar for rebuild and cleanup, not for migration', () => {
  assert.equal(jobProgressPercent(0, 0, 'completed'), 100)
  assert.equal(jobProgressPercent(0, 0, 'failed'), 0)
  assert.equal(jobProgressPercent(0, 0, 'running'), 0)
  assert.equal(jobProgressPercent(0, 0, undefined), 0)
  // 存储迁移的语义：total===0 一律 0
  assert.equal(progressPercent(0, 0), 0)
})

test('progress is rounded and clamped to 100 for both variants', () => {
  assert.equal(progressPercent(1, 3), 33)
  assert.equal(progressPercent(2, 3), 67)
  assert.equal(progressPercent(5, 4), 100, '后端计数瞬时超过总量时不能溢出')
  assert.equal(jobProgressPercent(1, 3, 'running'), 33)
  assert.equal(jobProgressPercent(5, 4, 'running'), 100)
  // total 非 0 时状态不参与计算
  assert.equal(jobProgressPercent(1, 2, 'completed'), 50)
})

test('migration status text prefers the cleanup phase over the job status', () => {
  assert.equal(migrationStatusText({ status: 'running', cleanupStatus: 'cleaning' }), '正在清理旧存储')
  assert.equal(migrationStatusText({ status: 'completed', cleanupStatus: 'not_ready' }), '迁移完成')
  assert.equal(migrationStatusText({ status: 'completed', cleanupStatus: 'pending' }), '等待处理旧存储')
  assert.equal(migrationStatusText({ status: 'completed', cleanupStatus: 'cleaned' }), '旧存储已清理')
  assert.equal(migrationStatusText({ status: 'completed', cleanupStatus: 'retained' }), '旧存储已保留')
  assert.equal(migrationStatusText({ status: 'completed', cleanupStatus: 'failed' }), '旧存储清理失败')
  assert.equal(migrationStatusText({ status: 'running', cleanupStatus: 'not_ready' }), '正在迁移')
  assert.equal(migrationStatusText({ status: 'interrupted', cleanupStatus: 'not_ready' }), '迁移被重启中断')
})

test('migration tone: cleaned/retained win over a failed cleanup', () => {
  assert.equal(migrationStatusTone({ status: 'completed', cleanupStatus: 'cleaned' }), 'success')
  assert.equal(migrationStatusTone({ status: 'completed', cleanupStatus: 'retained' }), 'success')
  assert.equal(migrationStatusTone({ status: 'failed', cleanupStatus: 'not_ready' }), 'danger')
  assert.equal(migrationStatusTone({ status: 'completed', cleanupStatus: 'failed' }), 'danger')
  assert.equal(migrationStatusTone({ status: 'cancelled', cleanupStatus: 'not_ready' }), 'warning')
  assert.equal(migrationStatusTone({ status: 'completed', cleanupStatus: 'interrupted' }), 'warning')
  assert.equal(migrationStatusTone({ status: 'running', cleanupStatus: 'not_ready' }), 'accent')
})

test('rebuild status distinguishes the clearing phase from generating', () => {
  assert.equal(thumbnailStatusText(null), '尚未手动重建')
  assert.equal(thumbnailStatusText({ status: 'running', phase: 'clearing' }), '正在清空缓存')
  assert.equal(thumbnailStatusText({ status: 'running', phase: 'generating' }), '正在并发生成')
  assert.equal(thumbnailStatusText({ status: 'failed', phase: 'generating' }), '部分生成失败')
  assert.equal(thumbnailStatusText({ status: 'interrupted', phase: 'generating' }), '服务重启后待恢复')
  assert.equal(thumbnailStatusTone(null), 'default')
  assert.equal(thumbnailStatusTone({ status: 'completed', phase: 'generating' }), 'success')
  assert.equal(thumbnailStatusTone({ status: 'cancelled', phase: 'generating' }), 'warning')
})

test('an empty S3 bucket reads as clean, a non-empty scan awaits confirmation', () => {
  assert.equal(s3CleanupStatusText(null), '尚未扫描')
  assert.equal(s3CleanupStatusText({ status: 'running', phase: 'scanning', total: 0 }), '正在扫描对象')
  assert.equal(s3CleanupStatusText({ status: 'running', phase: 'deleting', total: 5 }), '正在并发清理')
  assert.equal(s3CleanupStatusText({ status: 'ready', phase: 'ready', total: 0 }), '空间干净')
  assert.equal(s3CleanupStatusText({ status: 'ready', phase: 'ready', total: 7 }), '等待确认清理')
  // 「空间干净」是成功态，「等待确认清理」是需要处理的警告态
  assert.equal(s3CleanupStatusTone({ status: 'ready', phase: 'ready', total: 0 }), 'success')
  assert.equal(s3CleanupStatusTone({ status: 'ready', phase: 'ready', total: 7 }), 'warning')
  assert.equal(s3CleanupStatusTone({ status: 'completed', phase: 'ready', total: 7 }), 'success')
})

// 1024 进制，且 >=100 或 B 单位时不带小数（与重写前逐位一致）。
test('byte sizes use 1024 steps and drop decimals at three digits', () => {
  assert.equal(formatBytes(0), '0 B')
  assert.equal(formatBytes(-1), '0 B')
  assert.equal(formatBytes(NaN), '0 B')
  assert.equal(formatBytes(512), '512 B')
  assert.equal(formatBytes(1024), '1.0 KB')
  assert.equal(formatBytes(1536), '1.5 KB')
  assert.equal(formatBytes(1024 * 150), '150 KB')
  assert.equal(formatBytes(1024 ** 3 * 2.5), '2.5 GB')
  // 超过 TB 仍停在 TB（unit 上限 4）
  assert.equal(formatBytes(1024 ** 5), '1024 TB')
})
