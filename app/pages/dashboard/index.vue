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
const terminalJobs = computed(() =>
  jobs.value.filter(job =>
    ['completed', 'failed', 'cancelled', 'interrupted'].includes(job.status),
  ),
)
const recentJobs = computed(() => jobs.value.slice(0, 6))

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
  <UDashboardPanel>
    <template #header>
      <UDashboardNavbar title="概览">
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
      <div class="flex flex-col gap-6">
        <UAlert
          v-if="loadError"
          color="error"
          variant="subtle"
          icon="tabler:alert-circle"
          title="概览数据加载失败"
          :description="loadError"
        />

        <div class="grid grid-cols-2 gap-4 lg:grid-cols-4">
          <UCard class="cursor-pointer transition hover:ring-primary/40" @click="$router.push('/dashboard/albums')">
            <div class="flex items-center justify-between gap-4">
              <div>
                <p class="text-sm text-muted">相簿空间</p>
                <p class="mt-1 text-2xl font-semibold">{{ albums.length }}</p>
              </div>
              <span class="flex size-10 items-center justify-center rounded-lg bg-info/10 text-info">
                <Icon name="tabler:album" class="size-6" />
              </span>
            </div>
          </UCard>

          <UCard class="cursor-pointer transition hover:ring-primary/40" @click="$router.push('/dashboard/albums')">
            <div class="flex items-center justify-between gap-4">
              <div>
                <p class="text-sm text-muted">相片总数</p>
                <p class="mt-1 text-2xl font-semibold">{{ photos.length }}</p>
              </div>
              <span class="flex size-10 items-center justify-center rounded-lg bg-success/10 text-success">
                <Icon name="tabler:photo" class="size-6" />
              </span>
            </div>
          </UCard>

          <UCard class="cursor-pointer transition hover:ring-primary/40" @click="$router.push('/dashboard/conversions')">
            <div class="flex items-center justify-between gap-4">
              <div>
                <p class="text-sm text-muted">活动转换任务</p>
                <p class="mt-1 text-2xl font-semibold">{{ activeJobs.length }}</p>
              </div>
              <span class="flex size-10 items-center justify-center rounded-lg bg-warning/10 text-warning">
                <Icon name="tabler:loader-2" class="size-6" />
              </span>
            </div>
          </UCard>

          <UCard class="cursor-pointer transition hover:ring-primary/40" @click="$router.push('/dashboard/settings/storage')">
            <div class="flex items-center justify-between gap-4">
              <div class="min-w-0">
                <p class="text-sm text-muted">当前存储</p>
                <p class="mt-1 truncate text-lg font-semibold">
                  {{ storage ? storageLabels[storage.backend] : '—' }}
                </p>
              </div>
              <span class="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
                <Icon name="tabler:database" class="size-6" />
              </span>
            </div>
          </UCard>
        </div>

        <div class="grid grid-cols-1 gap-4 xl:grid-cols-5">
          <UCard class="xl:col-span-3">
            <template #header>
              <div class="flex items-center justify-between gap-4">
                <div>
                  <h2 class="font-semibold">最近转换任务</h2>
                  <p class="mt-1 text-sm text-muted">后台异步执行，关闭页面不会停止任务</p>
                </div>
                <UBadge color="neutral" variant="soft">
                  {{ terminalJobs.length }} 个终态任务
                </UBadge>
              </div>
            </template>

            <div v-if="recentJobs.length" class="divide-y divide-default">
              <button
                v-for="job in recentJobs"
                :key="job.id"
                type="button"
                class="flex w-full items-center gap-4 py-3 text-left first:pt-0 last:pb-0"
                @click="$router.push('/dashboard/conversions')"
              >
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span class="font-medium">{{ job.targetFormat.toUpperCase() }}</span>
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
              </button>
            </div>

            <div v-else class="flex min-h-40 flex-col items-center justify-center text-center">
              <Icon name="tabler:arrows-exchange" class="size-9 text-muted" />
              <p class="mt-3 font-medium">还没有转换任务</p>
              <p class="mt-1 text-sm text-muted">请先在相簿中上传图片</p>
            </div>
          </UCard>

          <UCard class="xl:col-span-2">
            <template #header>
              <h2 class="font-semibold">管理流程</h2>
            </template>

            <ol class="space-y-5">
              <li class="flex gap-3">
                <span class="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary/10 text-sm font-semibold text-primary">1</span>
                <div>
                  <p class="font-medium">创建相簿空间</p>
                  <p class="mt-1 text-sm text-muted">没有相簿时不能上传图片。</p>
                </div>
              </li>
              <li class="flex gap-3">
                <span class="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary/10 text-sm font-semibold text-primary">2</span>
                <div>
                  <p class="font-medium">选中相簿并上传</p>
                  <p class="mt-1 text-sm text-muted">仅接受 PNG、JPG/JPEG 和 WEBP。</p>
                </div>
              </li>
              <li class="flex gap-3">
                <span class="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary/10 text-sm font-semibold text-primary">3</span>
                <div>
                  <p class="font-medium">异步转换与人工确认</p>
                  <p class="mt-1 text-sm text-muted">先验收转换结果，再由管理员确认是否删除旧格式原图。</p>
                </div>
              </li>
            </ol>
          </UCard>
        </div>
      </div>
    </template>
  </UDashboardPanel>
</template>
