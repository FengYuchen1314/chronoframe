<script setup lang="ts">
import { Alert as AAlert, Button as AButton, Card as ACard, Empty as AEmpty, Progress as AProgress, RadioGroup as ARadioGroup, Space as ASpace, Tag as ATag } from 'ant-design-vue'
import type { AdminAlbumDownloads } from '~~/shared/types/downloads'
import type { S3CleanupJob, StorageMigrationJob, ThumbnailRebuildJob } from '~/types/dashboard'
definePageMeta({ layout: 'dashboard' })
useHead({ title: '任务中心' })
const { adminFetch } = useAdminApi()
const uploads = useAdminUploads()
type Task = { id: string, title: string, status: string, group: 'active' | 'attention' | 'finished', completed: number, total: number, updatedAt: number, error: string | null, link: { path: string, query: Record<string, string> } }
const downloads = ref<AdminAlbumDownloads | null>(null)
const migration = ref<StorageMigrationJob | null>(null)
const thumbnail = ref<ThumbnailRebuildJob | null>(null)
const cleanup = ref<S3CleanupJob | null>(null)
const errors = ref<string[]>([])
const loading = ref(false)
const filter = ref('all')
const groupOf = (status: string): Task['group'] => ['queued', 'running', 'deleting'].includes(status) ? 'active' : ['failed', 'interrupted', 'pending', 'confirm'].includes(status) ? 'attention' : 'finished'
const tasks = computed<Task[]>(() => {
  const result: Task[] = []
  for (const job of downloads.value?.jobs || []) {
    const config = downloads.value?.settings.find(item => item.albumId === job.albumId)
    if (!config?.enabled || config.revision !== job.revision || job.status === 'deleted') continue
    result.push({ ...job, title: `${job.albumName} · ${job.format.toUpperCase()} 下载包`, group: groupOf(job.status), link: { path: '/dashboard/albums', query: { album: job.albumId, tab: 'downloads' } } })
  }
  const m = migration.value
  if (m) {
    const cleaning = m.status === 'completed' && m.cleanupStatus === 'cleaning'
    const status = m.status === 'completed' && ['pending', 'failed', 'interrupted'].includes(m.cleanupStatus) ? 'confirm' : cleaning ? 'running' : m.status
    result.push({ ...m, status, title: `存储迁移 · ${m.sourceBackend.toUpperCase()} → ${m.targetBackend.toUpperCase()}${cleaning ? ' · 清理旧副本' : ''}`, completed: cleaning ? m.cleanupCompleted : m.completed, group: groupOf(status), link: { path: '/dashboard/settings/storage', query: { tab: 'migration' } } })
  }
  if (thumbnail.value) result.push({ ...thumbnail.value, title: '三层图片缓存重建', group: groupOf(thumbnail.value.status), link: { path: '/dashboard/settings/storage', query: { tab: 'cache' } } })
  if (cleanup.value) {
    const status = cleanup.value.status === 'ready' && cleanup.value.total > 0 ? 'confirm' : cleanup.value.status
    result.push({ ...cleanup.value, status, title: 'S3 旧对象清理', group: groupOf(status), link: { path: '/dashboard/settings/storage', query: { tab: 'cleanup' } } })
  }
  const rank = { attention: 0, active: 1, finished: 2 }
  return result.sort((a, b) => rank[a.group] - rank[b.group] || b.updatedAt - a.updatedAt)
})
const filtered = computed(() => tasks.value.filter(task => filter.value === 'all' || task.group === filter.value))
const filters = computed(() => [{ label: `全部 ${tasks.value.length}`, value: 'all' }, { label: `进行中 ${tasks.value.filter(task => task.group === 'active').length}`, value: 'active' }, { label: `需处理 ${tasks.value.filter(task => task.group === 'attention').length}`, value: 'attention' }, { label: '已结束', value: 'finished' }])
const labels: Record<string, string> = { queued: '等待运行', running: '运行中', ready: '已就绪', failed: '失败', interrupted: '已中断', completed: '已完成', cancelled: '已取消', deleting: '清理中', confirm: '等待确认旧文件处理' }
let timer: ReturnType<typeof setTimeout> | undefined
let mounted = false
const load = async () => {
  if (loading.value) return
  loading.value = true
  const messages: string[] = []
  await Promise.all([
    adminFetch<AdminAlbumDownloads>('/api/album-downloads').then(value => { downloads.value = value }).catch(cause => messages.push(`下载任务：${getAdminApiErrorMessage(cause)}`)),
    adminFetch<StorageMigrationJob[]>('/api/storage-migrations').then(value => { migration.value = value[0] || null }).catch(cause => messages.push(`迁移任务：${getAdminApiErrorMessage(cause)}`)),
    adminFetch<ThumbnailRebuildJob | null>('/api/thumbnails/rebuilds/latest').then(value => { thumbnail.value = value }).catch(cause => messages.push(`缓存任务：${getAdminApiErrorMessage(cause)}`)),
    adminFetch<S3CleanupJob | null>('/api/s3-cleanups/latest').then(value => { cleanup.value = value }).catch(cause => messages.push(`清理任务：${getAdminApiErrorMessage(cause)}`)),
  ])
  errors.value = messages
  loading.value = false
}
const poll = async () => { await load(); if (mounted) timer = setTimeout(poll, document.hidden ? 15000 : 5000) }
onMounted(() => { mounted = true; void poll() })
onBeforeUnmount(() => { mounted = false; clearTimeout(timer) })
</script>

<template>
  <div>
    <DashboardPageHeader title="任务中心" description="服务器任务独立运行，不需要停留在此页面。需确认和失败的任务优先显示。"><AButton :loading="loading" @click="load">刷新</AButton></DashboardPageHeader>
    <div class="admin-stack">
      <AAlert v-if="errors.length" type="warning" show-icon message="部分任务状态暂时无法更新，保留上次结果" :description="errors.join('\n')" />
      <ACard v-if="uploads.state.value.items.length" size="small"><div class="admin-toolbar" style="margin:0"><div><strong>本浏览器的上传队列</strong><p class="admin-help" style="margin:4px 0 0">已入库 {{ uploads.done.value }} · 上传中 {{ uploads.active.value }} · 排队 {{ uploads.queued.value }} · 未确认 {{ uploads.failed.value }}。切换后台页面不影响上传，关闭浏览器会停止。</p></div><AButton @click="uploads.open.value = true">查看上传队列</AButton></div></ACard>
      <ARadioGroup v-model:value="filter" :options="filters" option-type="button" aria-label="任务状态筛选" />
      <AEmpty v-if="!filtered.length && !loading" description="这里暂时没有任务" />
      <ACard v-for="task in filtered" :key="task.id" size="small">
        <div class="admin-toolbar"><strong>{{ task.title }}</strong><ASpace><ATag :color="task.group === 'attention' ? 'orange' : task.group === 'active' ? 'processing' : 'default'">{{ labels[task.status] || task.status }}</ATag><NuxtLink :to="task.link">{{ task.group === 'attention' ? '去处理 →' : '查看详情 →' }}</NuxtLink></ASpace></div>
        <AProgress v-if="task.total" :percent="Math.min(100, Math.round(task.completed / task.total * 100))" :status="task.group === 'attention' ? 'exception' : undefined" size="small" />
        <div class="admin-toolbar" style="margin:4px 0 0"><span class="admin-help">{{ task.completed }} / {{ task.total }}</span><span class="admin-help">更新于 {{ new Date(task.updatedAt * 1000).toLocaleString('zh-CN', { hour12: false }) }}</span></div>
        <p v-if="task.error" class="admin-field-error">{{ task.error }}</p>
      </ACard>
      <p class="admin-help">下载任务只展示当前版本；历史 ZIP 在对应相册的“公开下载 → 显示历史记录”中查看。迁移、缓存和 S3 清理展示各自最近一次任务。</p>
    </div>
  </div>
</template>
