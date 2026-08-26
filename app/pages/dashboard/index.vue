<script lang="ts" setup>
import type {
  Album,
  ConversionJob,
  Photo,
  StorageBackend,
  StorageSettings,
} from '~/types/dashboard'

definePageMeta({
  layout: 'dashboard',
})

useHead({ title: '概览' })

type BadgeColor = 'neutral' | 'info' | 'success' | 'warning' | 'error'

const { adminFetch } = useAdminApi()
const albums = ref<Album[]>([])
const photos = ref<Photo[]>([])
const jobs = ref<ConversionJob[]>([])
const storage = ref<StorageSettings | null>(null)
const isLoading = ref(false)
const loadError = ref('')

const activeJobs = computed(() =>
  jobs.value.filter(job => ['queued', 'running'].includes(job.status)),
)
const recentJobs = computed(() => jobs.value.slice(0, 6))
const failedJobs = computed(() =>
  jobs.value.filter(job => ['failed', 'interrupted'].includes(job.status)),
)
const totalBytes = computed(() =>
  photos.value.reduce((total, photo) => total + photo.byteSize, 0),
)

const storageLabels: Record<StorageBackend, string> = {
  local: '本地存储',
  webdav: 'WebDAV',
  s3: 'S3 对象存储',
}

const statusLabels: Record<string, string> = {
  queued: '排队中',
  running: '转换中',
  completed: '已完成',
  failed: '失败',
  cancelled: '已中断',
  interrupted: '异常中断',
}

const statusColors: Record<string, BadgeColor> = {
  queued: 'warning',
  running: 'info',
  completed: 'success',
  failed: 'error',
  cancelled: 'neutral',
  interrupted: 'error',
}

const statusLabel = (status: string) => statusLabels[status] || status
const statusColor = (status: string): BadgeColor => statusColors[status] || 'neutral'

const progressOf = (job: ConversionJob) =>
  job.total > 0 ? Math.round((job.completed / job.total) * 100) : 0

const formatTime = (timestamp: number) =>
  new Date(timestamp * 1000).toLocaleString('zh-CN', { hour12: false })

const formatBytes = (bytes: number) => {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 1)} ${units[index]}`
}

const refreshAll = async () => {
  if (isLoading.value) return
  isLoading.value = true
  loadError.value = ''

  try {
    const [albumList, photoList, jobList, storageSettings] = await Promise.all([
      adminFetch<Album[]>('/api/albums'),
      adminFetch<Photo[]>('/api/photos'),
      adminFetch<ConversionJob[]>('/api/conversions'),
      adminFetch<StorageSettings>('/api/settings/storage'),
    ])

    albums.value = albumList
    photos.value = photoList
    jobs.value = jobList
    storage.value = storageSettings
  } catch (error) {
    loadError.value = getAdminApiErrorMessage(error)
  } finally {
    isLoading.value = false
  }
}

onMounted(refreshAll)
</script>

<template>
  <UDashboardPanel :ui="{ body: 'p-0 sm:p-0' }">
    <template #header>
      <UDashboardNavbar title="工作台">
        <template #right>
          <UButton
            icon="tabler:refresh"
            color="neutral"
            variant="ghost"
            :loading="isLoading"
            @click="refreshAll"
          >
            刷新
          </UButton>
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="dashboard-panel-body flex flex-col gap-6">
        <UAlert
          v-if="loadError"
          color="error"
          variant="subtle"
          icon="tabler:alert-circle"
          title="概览数据加载失败"
          :description="loadError"
        />

        <DashboardPageHero
          eyebrow="工作台"
          title="内容与任务概览"
          description="查看相簿、图片、转换任务和当前存储状态，直接进入需要处理的工作区。"
          icon="tabler:layout-dashboard"
        >
          <template #actions>
            <UButton to="/dashboard/albums" icon="tabler:plus">新建相簿</UButton>
            <UButton to="/dashboard/conversions" color="neutral" variant="soft" icon="tabler:arrows-exchange">格式转换</UButton>
          </template>
        </DashboardPageHero>

        <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <DashboardMetricCard label="相簿空间" :value="albums.length" icon="tabler:album" tone="info" hint="进入相簿工作台" to="/dashboard/albums" />
          <DashboardMetricCard label="图片总数" :value="photos.length" icon="tabler:photo" tone="success" :hint="formatBytes(totalBytes)" to="/dashboard/albums" />
          <DashboardMetricCard label="进行中任务" :value="activeJobs.length" icon="tabler:progress" tone="warning" hint="后台异步执行" to="/dashboard/conversions" />
          <DashboardMetricCard label="当前存储" :value="storage ? storageLabels[storage.backend] : '—'" icon="tabler:database" tone="neutral" :hint="failedJobs.length ? `${failedJobs.length} 个异常任务待查看` : '运行状态正常'" to="/dashboard/settings/storage" />
        </div>

        <div class="grid grid-cols-1 gap-4 xl:grid-cols-5">
          <section class="dashboard-section overflow-hidden xl:col-span-3">
            <div class="flex items-center justify-between gap-4 border-b border-default px-5 py-4">
              <div>
                <h2 class="font-semibold text-highlighted">最近转换任务</h2>
                <p class="mt-1 text-sm text-muted">任务在后端执行，离开页面也不会停止</p>
              </div>
              <div class="flex items-center gap-2"><UBadge v-if="failedJobs.length" color="warning" variant="soft">{{ failedJobs.length }} 个需关注</UBadge><UButton to="/dashboard/conversions" color="neutral" variant="ghost" trailing-icon="tabler:arrow-right">全部任务</UButton></div>
            </div>

            <div v-if="recentJobs.length" class="divide-y divide-default px-5">
              <button
                v-for="job in recentJobs"
                :key="job.id"
                type="button"
                class="group flex w-full items-center gap-4 py-4 text-left"
                @click="$router.push('/dashboard/conversions')"
              >
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span class="font-medium text-highlighted">转为 {{ job.targetFormat.toUpperCase() }}</span>
                    <UBadge :color="statusColor(job.status)" variant="soft" size="sm">
                      {{ statusLabel(job.status) }}
                    </UBadge>
                  </div>
                  <div class="mt-2 flex items-center gap-3">
                    <UProgress :model-value="progressOf(job)" class="flex-1" />
                    <span class="w-12 text-right text-xs text-muted">{{ progressOf(job) }}%</span>
                  </div>
                </div>
                <div class="hidden text-right text-xs text-muted sm:block">
                  <p>{{ job.completed }} / {{ job.total }}</p>
                  <p class="mt-1">{{ formatTime(job.updatedAt) }}</p>
                </div>
                <Icon name="tabler:chevron-right" class="size-4 text-dimmed transition group-hover:translate-x-0.5 group-hover:text-primary" />
              </button>
            </div>

            <div v-else class="flex min-h-52 flex-col items-center justify-center px-5 text-center">
              <Icon name="tabler:arrows-exchange" class="size-9 text-muted" />
              <p class="mt-3 font-medium">还没有转换任务</p>
              <p class="mt-1 text-sm text-muted">上传图片后，可以批量转换一个或多个相簿</p>
            </div>
          </section>

          <section class="dashboard-section overflow-hidden xl:col-span-2">
            <div class="border-b border-default px-5 py-4">
              <h2 class="font-semibold text-highlighted">快捷操作</h2>
              <p class="mt-1 text-sm text-muted">按当前任务直接进入对应工作区</p>
            </div>
            <div class="space-y-2 p-3">
              <NuxtLink to="/dashboard/albums" class="group flex items-center gap-3 rounded-xl p-3 transition hover:bg-elevated">
                <span class="flex size-10 items-center justify-center rounded-xl bg-primary/10 text-primary"><Icon name="tabler:folder-plus" class="size-5" /></span>
                <span class="min-w-0 flex-1"><span class="block font-medium text-highlighted">创建相簿并上传</span><span class="mt-0.5 block text-xs text-muted">管理简介、日期、排序和内容</span></span>
                <Icon name="tabler:chevron-right" class="size-4 text-dimmed group-hover:text-primary" />
              </NuxtLink>
              <NuxtLink to="/dashboard/conversions" class="group flex items-center gap-3 rounded-xl p-3 transition hover:bg-elevated">
                <span class="flex size-10 items-center justify-center rounded-xl bg-warning/10 text-warning"><Icon name="tabler:arrows-exchange" class="size-5" /></span>
                <span class="min-w-0 flex-1"><span class="block font-medium text-highlighted">批量格式转换</span><span class="mt-0.5 block text-xs text-muted">查看进度并人工确认旧图</span></span>
                <Icon name="tabler:chevron-right" class="size-4 text-dimmed group-hover:text-primary" />
              </NuxtLink>
              <NuxtLink to="/dashboard/settings/storage" class="group flex items-center gap-3 rounded-xl p-3 transition hover:bg-elevated">
                <span class="flex size-10 items-center justify-center rounded-xl bg-info/10 text-info"><Icon name="tabler:database-cog" class="size-5" /></span>
                <span class="min-w-0 flex-1"><span class="block font-medium text-highlighted">检查存储连接</span><span class="mt-0.5 block text-xs text-muted">{{ storage ? storageLabels[storage.backend] : '读取中' }} · 配置保存在后台</span></span>
                <Icon name="tabler:chevron-right" class="size-4 text-dimmed group-hover:text-primary" />
              </NuxtLink>
            </div>
            <div class="mx-5 mb-5 flex items-center gap-2 rounded-xl bg-success/10 px-3 py-2.5 text-xs text-success">
              <Icon name="tabler:shield-check" class="size-4 shrink-0" />
              旧格式原图不会被自动删除
            </div>
          </section>
        </div>
      </div>
    </template>
  </UDashboardPanel>
</template>
