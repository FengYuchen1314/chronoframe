import { useCallback, useEffect, useRef, useState } from 'react'
import { Link } from 'react-router-dom'
import { Alert, Button, Card, Chip, ProgressBar } from '@heroui/react'
import { getAdminApiErrorMessage, adminApi } from '../lib/api'
import { useUploads } from '../lib/store'
import { formatDateTime, progressPercent } from '../lib/format'
import { Icon } from '../lib/icons'
import type {
  AdminAlbumDownloads,
  S3CleanupJob,
  StorageMigrationJob,
  ThumbnailRebuildJob,
} from '../lib/types'
import PageHeader from '../components/PageHeader'
import { useDocumentTitle } from '../lib/hooks'

type TaskGroup = 'active' | 'attention' | 'finished'

interface Task {
  id: string
  title: string
  status: string
  group: TaskGroup
  completed: number
  total: number
  updatedAt: number
  error: string | null
  link: string
}

const groupOf = (status: string): TaskGroup =>
  ['queued', 'running', 'deleting'].includes(status) ? 'active'
    : ['failed', 'interrupted', 'pending', 'confirm'].includes(status) ? 'attention'
      : 'finished'

const STATUS_LABELS: Record<string, string> = {
  queued: '等待运行',
  running: '运行中',
  ready: '已就绪',
  failed: '失败',
  interrupted: '已中断',
  completed: '已完成',
  cancelled: '已取消',
  deleting: '清理中',
  confirm: '等待确认旧文件处理',
}

const GROUP_RANK: Record<TaskGroup, number> = { attention: 0, active: 1, finished: 2 }

const FILTERS: Array<{ value: 'all' | TaskGroup; label: string }> = [
  { value: 'all', label: '全部' },
  { value: 'active', label: '进行中' },
  { value: 'attention', label: '需处理' },
  { value: 'finished', label: '已结束' },
]

export default function Tasks() {
  useDocumentTitle('任务中心')
  const uploads = useUploads()
  const [downloads, setDownloads] = useState<AdminAlbumDownloads | null>(null)
  const [migration, setMigration] = useState<StorageMigrationJob | null>(null)
  const [thumbnail, setThumbnail] = useState<ThumbnailRebuildJob | null>(null)
  const [cleanup, setCleanup] = useState<S3CleanupJob | null>(null)
  const [errors, setErrors] = useState<string[]>([])
  const [loading, setLoading] = useState(false)
  const [filter, setFilter] = useState<'all' | TaskGroup>('all')

  // 递归 setTimeout（不是 setInterval）：慢请求不会堆积。仅卸载时停止。
  const mounted = useRef(false)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const loadingRef = useRef(false)

  // silent=true 时不切换 loading：后台轮询不应该让「刷新」按钮每 5 秒闪一次，
  // 也不应该把空态一直压住。
  const load = useCallback(async (silent = false) => {
    if (loadingRef.current) return
    loadingRef.current = true
    if (!silent) setLoading(true)
    const messages: string[] = []

    await Promise.all([
      adminApi.adminFetch<AdminAlbumDownloads>('/api/album-downloads')
        .then((value) => setDownloads(value))
        .catch((cause) => messages.push(`下载任务：${getAdminApiErrorMessage(cause)}`)),
      adminApi.adminFetch<StorageMigrationJob[]>('/api/storage-migrations')
        .then((value) => setMigration(value[0] || null))
        .catch((cause) => messages.push(`迁移任务：${getAdminApiErrorMessage(cause)}`)),
      adminApi.adminFetch<ThumbnailRebuildJob | null>('/api/thumbnails/rebuilds/latest')
        .then((value) => setThumbnail(value))
        .catch((cause) => messages.push(`缓存任务：${getAdminApiErrorMessage(cause)}`)),
      adminApi.adminFetch<S3CleanupJob | null>('/api/s3-cleanups/latest')
        .then((value) => setCleanup(value))
        .catch((cause) => messages.push(`清理任务：${getAdminApiErrorMessage(cause)}`)),
    ])

    // 失败时保留上一次的值，只提示部分状态未能更新。
    setErrors(messages)
    if (!silent) setLoading(false)
    loadingRef.current = false
  }, [])

  useEffect(() => {
    mounted.current = true
    const poll = async () => {
      await load(true)
      if (!mounted.current) return
      timer.current = setTimeout(() => void poll(), document.hidden ? 15000 : 5000)
    }
    void poll()
    return () => {
      mounted.current = false
      clearTimeout(timer.current)
    }
  }, [load])

  const tasks: Task[] = []
  for (const job of downloads?.jobs || []) {
    const config = downloads?.settings.find((item) => item.albumId === job.albumId)
    if (!config?.enabled || config.revision !== job.revision || job.status === 'deleted') continue
    tasks.push({
      ...job,
      title: `${job.albumName} · ${job.format.toUpperCase()} 下载包`,
      group: groupOf(job.status),
      link: `/albums?album=${encodeURIComponent(job.albumId)}&tab=downloads`,
    })
  }

  if (migration) {
    const cleaning = migration.status === 'completed' && migration.cleanupStatus === 'cleaning'
    const status = migration.status === 'completed'
      && ['pending', 'failed', 'interrupted'].includes(migration.cleanupStatus)
      ? 'confirm'
      : cleaning ? 'running' : migration.status
    tasks.push({
      ...migration,
      status,
      title: `存储迁移 · ${migration.sourceBackend.toUpperCase()} → ${migration.targetBackend.toUpperCase()}${cleaning ? ' · 清理旧副本' : ''}`,
      completed: cleaning ? migration.cleanupCompleted : migration.completed,
      group: groupOf(status),
      link: '/settings/storage?tab=migration',
    })
  }

  if (thumbnail) {
    tasks.push({
      ...thumbnail,
      title: '三层图片缓存重建',
      group: groupOf(thumbnail.status),
      link: '/settings/storage?tab=cache',
    })
  }

  if (cleanup) {
    const status = cleanup.status === 'ready' && cleanup.total > 0 ? 'confirm' : cleanup.status
    tasks.push({
      ...cleanup,
      status,
      title: 'S3 旧对象清理',
      group: groupOf(status),
      link: '/settings/storage?tab=cleanup',
    })
  }

  // 需处理排最前，其次进行中，最后已结束；组内按更新时间倒序。
  tasks.sort((a, b) => GROUP_RANK[a.group] - GROUP_RANK[b.group] || b.updatedAt - a.updatedAt)

  const filtered = tasks.filter((task) => filter === 'all' || task.group === filter)
  const counts: Record<'all' | TaskGroup, number> = {
    all: tasks.length,
    active: tasks.filter((task) => task.group === 'active').length,
    attention: tasks.filter((task) => task.group === 'attention').length,
    finished: tasks.filter((task) => task.group === 'finished').length,
  }

  return (
    <div>
      <PageHeader
        title="任务中心"
        description="服务器任务独立运行，不需要停留在此页面。需确认和失败的任务优先显示。"
        actions={
          <Button variant="secondary" isDisabled={loading} onPress={() => void load()}>
            <Icon name="refresh" size={16} />
            刷新
          </Button>
        }
      />

      <div className="admin-stack">
        {errors.length ? (
          <Alert status="warning">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Title>部分任务状态暂时无法更新，保留上次结果</Alert.Title>
              <Alert.Description>
                <span className="preserve-newlines">{errors.join('\n')}</span>
              </Alert.Description>
            </Alert.Content>
          </Alert>
        ) : null}

        {uploads.items.length ? (
          <Card>
            <Card.Content>
              <div className="admin-toolbar">
                <div>
                  <strong>本浏览器的上传队列</strong>
                  <p className="admin-help" style={{ marginTop: 4 }}>
                    已入库 {uploads.done} · 上传中 {uploads.active} · 排队 {uploads.queued} · 未确认{' '}
                    {uploads.failed}。切换后台页面不影响上传，关闭浏览器会停止。
                  </p>
                </div>
                <Button variant="secondary" onPress={() => uploads.setOpen(true)}>
                  查看上传队列
                </Button>
              </div>
            </Card.Content>
          </Card>
        ) : null}

        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
          {FILTERS.map((item) => {
            const count = item.value === 'all' ? counts.all : counts[item.value as TaskGroup]
            const label = item.value === 'finished' ? item.label : `${item.label} ${count}`
            return (
              <Button
                key={item.value}
                variant={filter === item.value ? 'primary' : 'secondary'}
                size="sm"
                onPress={() => setFilter(item.value)}
              >
                {label}
              </Button>
            )
          })}
        </div>

        {!filtered.length && !loading ? (
          <div style={{ display: 'grid', placeItems: 'center', gap: 10, padding: '48px 0', color: 'var(--muted)' }}>
            <Icon name="inbox" size={32} />
            <span>这里暂时没有任务</span>
          </div>
        ) : null}

        {filtered.map((task) => (
          <Card key={task.id}>
            <Card.Content>
              <div className="admin-toolbar">
                <strong>{task.title}</strong>
                <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
                  <Chip
                    color={task.group === 'attention' ? 'warning' : task.group === 'active' ? 'accent' : 'default'}
                    variant="soft"
                    size="sm"
                  >
                    {STATUS_LABELS[task.status] || task.status}
                  </Chip>
                  <Link
                    to={task.link}
                    style={{ color: 'var(--link)', fontSize: 14, whiteSpace: 'nowrap' }}
                  >
                    {task.group === 'attention' ? '去处理 →' : '查看详情 →'}
                  </Link>
                </div>
              </div>

              {task.total ? (
                <div style={{ marginTop: 12 }}>
                  <ProgressBar
                    value={progressPercent(task.completed, task.total)}
                    color={task.group === 'attention' ? 'danger' : 'accent'}
                    aria-label={`${task.title} 进度`}
                  >
                    <ProgressBar.Track>
                      <ProgressBar.Fill />
                    </ProgressBar.Track>
                  </ProgressBar>
                </div>
              ) : null}

              <div className="admin-toolbar" style={{ marginTop: 8 }}>
                <span className="admin-help">
                  {task.completed} / {task.total}
                </span>
                <span className="admin-help">更新于 {formatDateTime(task.updatedAt)}</span>
              </div>

              {task.error ? <p className="admin-field-error" style={{ marginTop: 8 }}>{task.error}</p> : null}
            </Card.Content>
          </Card>
        ))}

        <p className="admin-help">
          下载任务只展示当前版本；历史 ZIP 在对应相册的“公开下载 → 显示历史记录”中查看。迁移、缓存和 S3
          清理展示各自最近一次任务。
        </p>
      </div>
    </div>
  )
}
