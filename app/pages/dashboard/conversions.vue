<script lang="ts" setup>
import type {
  Album,
  ConversionDetail,
  ConversionJob,
  ImageTargetFormat,
  SourceDeletionResult,
} from '~/types/dashboard'
import { isAbortedRequest } from '~/utils/requestAbort'

definePageMeta({
  layout: 'dashboard',
})

useHead({ title: '格式转换' })

type BadgeColor = 'neutral' | 'info' | 'success' | 'warning' | 'error'

const ACTIVE_STATUSES = new Set(['queued', 'running'])
const TERMINAL_STATUSES = new Set(['completed', 'failed', 'cancelled', 'interrupted'])

const toast = useToast()
const { adminFetch } = useAdminApi()

const albums = ref<Album[]>([])
const selectedAlbumIds = ref<string[]>([])
const targetFormat = ref<ImageTargetFormat>('webp')
const jobs = ref<ConversionJob[]>([])
const selectedJobId = ref('')
const selectedDetail = ref<ConversionDetail | null>(null)
const deletionResults = ref<Record<string, SourceDeletionResult>>({})

const isLoading = ref(false)
const isStarting = ref(false)
const isLoadingDetail = ref(false)
const deletingJobId = ref('')
const cancellingJobIds = reactive(new Set<string>())
const fullDetailLoadedJobIds = reactive(new Set<string>())
const pageError = ref('')
const pollWarning = ref('')

const formatOptions = [
  { label: 'PNG', value: 'png' },
  { label: 'JPG', value: 'jpg' },
  { label: 'JPEG', value: 'jpeg' },
  { label: 'WEBP', value: 'webp' },
]

const selectedJob = computed(() =>
  jobs.value.find(job => job.id === selectedJobId.value) || null,
)
const detailJob = computed(() => selectedDetail.value?.job || selectedJob.value)
const hasActiveJobs = computed(() => jobs.value.some(job => isActive(job)))
const activeJobCount = computed(() => jobs.value.filter(job => isActive(job)).length)
const completedJobCount = computed(() => jobs.value.filter(job => job.status === 'completed').length)
const attentionJobCount = computed(() => jobs.value.filter(job => ['failed', 'interrupted'].includes(job.status)).length)
const convertedImageCount = computed(() => jobs.value.reduce((total, job) => total + job.succeeded, 0))
const hasPendingRecovery = computed(() => jobs.value.some(job => job.sourcesDeletedAt === -2))
const hasPollableWork = computed(() => hasActiveJobs.value || hasPendingRecovery.value)
const allAlbumsSelected = computed(() =>
  albums.value.length > 0 && selectedAlbumIds.value.length === albums.value.length,
)

const isActive = (job: ConversionJob) => ACTIVE_STATUSES.has(job.status)
const isTerminal = (job: ConversionJob) => TERMINAL_STATUSES.has(job.status)
const progressOf = (job: ConversionJob) =>
  job.total > 0 ? Math.min(100, Math.round((job.completed / job.total) * 100)) : 0

const statusLabels: Record<string, string> = {
  queued: '排队中',
  running: '转换中',
  completed: '已完成',
  failed: '失败',
  cancelled: '已安全中断',
  interrupted: '服务异常中断',
  processing: '处理中',
  succeeded: '成功',
}

const statusColors: Record<string, BadgeColor> = {
  queued: 'warning',
  running: 'info',
  completed: 'success',
  succeeded: 'success',
  failed: 'error',
  cancelled: 'neutral',
  interrupted: 'error',
  processing: 'info',
}

const statusLabel = (status: string) => statusLabels[status] || status
const statusColor = (status: string): BadgeColor => statusColors[status] || 'neutral'

const formatTime = (timestamp: number) =>
  new Date(timestamp * 1000).toLocaleString('zh-CN', { hour12: false })

const shortId = (id: string) => id.slice(0, 8)

const toggleAlbum = (albumId: string) => {
  selectedAlbumIds.value = selectedAlbumIds.value.includes(albumId)
    ? selectedAlbumIds.value.filter(id => id !== albumId)
    : [...selectedAlbumIds.value, albumId]
}

const toggleAllAlbums = () => {
  selectedAlbumIds.value = allAlbumsSelected.value
    ? []
    : albums.value.map(album => album.id)
}

let detailRequestSerial = 0
let detailController: AbortController | null = null

const loadJobDetail = async (jobId: string, includeItems?: boolean) => {
  const job = jobs.value.find(candidate => candidate.id === jobId)
  if (!job) {
    selectedDetail.value = null
    return
  }

  const requestSerial = ++detailRequestSerial
  const requestIncludesItems = includeItems ?? isTerminal(job)
  detailController?.abort()
  const controller = new AbortController()
  detailController = controller
  isLoadingDetail.value = true

  try {
    const detail = await adminFetch<ConversionDetail>(`/api/conversions/${jobId}`, {
      query: { items: requestIncludesItems },
      signal: controller.signal,
    })
    if (requestSerial === detailRequestSerial && selectedJobId.value === jobId) {
      selectedDetail.value = detail
      if (requestIncludesItems) fullDetailLoadedJobIds.add(jobId)
    }
  } catch (error) {
    if (!isAbortedRequest(error, controller.signal)) {
      toast.add({
        title: '任务详情加载失败',
        description: getAdminApiErrorMessage(error),
        color: 'error',
      })
    }
  } finally {
    if (requestSerial === detailRequestSerial) isLoadingDetail.value = false
    if (detailController === controller) detailController = null
  }
}

const applyJobList = (nextJobs: ConversionJob[]) => {
  jobs.value = nextJobs

  if (!nextJobs.some(job => job.id === selectedJobId.value)) {
    selectedJobId.value = nextJobs[0]?.id || ''
  }

  if (selectedDetail.value) {
    const current = nextJobs.find(job => job.id === selectedDetail.value?.job.id)
    if (current) selectedDetail.value = { ...selectedDetail.value, job: current }
  }

  for (const jobId of Array.from(cancellingJobIds)) {
    const job = nextJobs.find(candidate => candidate.id === jobId)
    if (!job || !isActive(job)) cancellingJobIds.delete(jobId)
  }
}

let pollTimer: ReturnType<typeof setTimeout> | null = null
let pollController: AbortController | null = null
let pollGeneration = 0
let consecutivePollFailures = 0

const clearPollTimer = () => {
  if (!pollTimer) return
  clearTimeout(pollTimer)
  pollTimer = null
}

const disposePolling = () => {
  pollGeneration += 1
  clearPollTimer()
  pollController?.abort()
  pollController = null
}

const schedulePoll = (generation: number, delay: number) => {
  if (!import.meta.client || generation !== pollGeneration || !hasPollableWork.value) return
  clearPollTimer()
  pollTimer = window.setTimeout(() => void pollJobs(generation), delay)
}

const beginPolling = (delay = 0) => {
  disposePolling()
  const generation = pollGeneration
  if (hasPollableWork.value) schedulePoll(generation, delay)
}

async function pollJobs(generation: number) {
  if (generation !== pollGeneration) return

  const controller = new AbortController()
  pollController = controller
  const previousSelectedStatus = selectedJob.value?.status

  try {
    const nextJobs = await adminFetch<ConversionJob[]>('/api/conversions', {
      signal: controller.signal,
    })
    if (generation !== pollGeneration) return

    applyJobList(nextJobs)
    consecutivePollFailures = 0
    pollWarning.value = ''

    const currentSelected = selectedJob.value
    if (
      currentSelected
      && isTerminal(currentSelected)
      && (
        !TERMINAL_STATUSES.has(previousSelectedStatus || '')
        || !fullDetailLoadedJobIds.has(currentSelected.id)
      )
    ) {
      await loadJobDetail(currentSelected.id, true)
    }
  } catch (error) {
    if (!isAbortedRequest(error, controller.signal)) {
      consecutivePollFailures += 1
      pollWarning.value = `进度同步暂时失败：${getAdminApiErrorMessage(error)}。已自动降低轮询频率。`
    }
  } finally {
    if (pollController === controller) pollController = null

    if (generation === pollGeneration && hasPollableWork.value) {
      const recoveryOnly = !hasActiveJobs.value && hasPendingRecovery.value
      const visibleDelay = document.visibilityState === 'hidden' ? 5000 : recoveryOnly ? 3000 : 1000
      const retryDelay = consecutivePollFailures
        ? Math.min(8000, 1000 * 2 ** consecutivePollFailures)
        : visibleDelay
      schedulePoll(generation, Math.max(visibleDelay, retryDelay))
    }
  }
}

const refreshPage = async () => {
  if (isLoading.value) return
  disposePolling()
  isLoading.value = true
  pageError.value = ''
  const selectedJobIdBeforeRefresh = selectedJobId.value

  try {
    const [albumList, jobList] = await Promise.all([
      adminFetch<Album[]>('/api/albums'),
      adminFetch<ConversionJob[]>('/api/conversions'),
    ])
    albums.value = albumList
    selectedAlbumIds.value = selectedAlbumIds.value.filter(id =>
      albumList.some(album => album.id === id),
    )
    applyJobList(jobList)

    // A changed selection is loaded by the watcher. Loading it here as well would
    // create two requests and immediately abort one of them on the initial refresh.
    if (selectedJobId.value && selectedJobId.value === selectedJobIdBeforeRefresh) {
      await loadJobDetail(selectedJobId.value)
    }
  } catch (error) {
    pageError.value = getAdminApiErrorMessage(error)
  } finally {
    isLoading.value = false
    beginPolling(1000)
  }
}

const startConversion = async () => {
  if (!selectedAlbumIds.value.length) {
    toast.add({ title: '至少选择一个相簿', color: 'warning' })
    return
  }

  isStarting.value = true
  try {
    const created = await adminFetch<ConversionJob>('/api/conversions', {
      method: 'POST',
      body: {
        albumIds: selectedAlbumIds.value,
        targetFormat: targetFormat.value,
      },
    })

    jobs.value = [created, ...jobs.value.filter(job => job.id !== created.id)]
    selectedJobId.value = created.id
    selectedDetail.value = { job: created, items: [] }
    toast.add({
      title: '转换任务已进入后台队列',
      description: `${created.total} 张图片将转换为 ${created.targetFormat.toUpperCase()}`,
      color: 'success',
    })
    beginPolling(0)
  } catch (error) {
    toast.add({
      title: '无法创建转换任务',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isStarting.value = false
  }
}

const requestCancellation = async (job: ConversionJob) => {
  if (!isActive(job) || cancellingJobIds.has(job.id)) return
  if (!window.confirm('确认安全中断该任务？\n\n已完成的转换结果会保留，未开始的项目会取消，且不会自动删除任何原图。')) return

  cancellingJobIds.add(job.id)
  try {
    await adminFetch(`/api/conversions/${job.id}/cancel`, { method: 'POST' })
    toast.add({
      title: '已提交安全中断请求',
      description: '界面会继续轮询，直到服务端写入最终状态。',
      color: 'warning',
    })
    beginPolling(0)
  } catch (error) {
    cancellingJobIds.delete(job.id)
    toast.add({
      title: '中断请求失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  }
}

const confirmSourceDeletion = async (job: ConversionJob) => {
  if (!isTerminal(job) || job.succeeded <= 0 || job.sourcesDeletedAt !== null) return
  const confirmed = window.confirm(
    `确认删除该任务中 ${job.succeeded} 张转换成功的旧格式原图？\n\n后端会先逐一验证新图存在且格式、大小正确，然后才删除旧图。此操作不可撤销。`,
  )
  if (!confirmed) return

  deletingJobId.value = job.id
  try {
    const result = await adminFetch<SourceDeletionResult>(
      `/api/conversions/${job.id}/delete-sources`,
      { method: 'DELETE' },
    )
    deletionResults.value = { ...deletionResults.value, [job.id]: result }
    toast.add({
      title: result.failures.length ? '旧图删除已持久化，但有项目待恢复' : '旧格式原图已删除',
      description: `已删除 ${result.removed} 张，失败 ${result.failures.length} 张`,
      color: result.failures.length ? 'warning' : 'success',
    })
    await loadJobDetail(job.id, true)
    const refreshedJob = selectedDetail.value?.job
    if (refreshedJob?.id === job.id) {
      jobs.value = jobs.value.map(candidate =>
        candidate.id === job.id ? refreshedJob : candidate,
      )
    }
    beginPolling(result.failures.length ? 3000 : 0)
  } catch (error) {
    toast.add({
      title: '旧图删除确认失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    deletingJobId.value = ''
  }
}

const selectJob = (jobId: string) => {
  if (selectedJobId.value === jobId) return
  selectedJobId.value = jobId
}

const handleVisibilityChange = () => {
  if (document.visibilityState === 'visible' && hasPollableWork.value) beginPolling(0)
}

watch(selectedJobId, (jobId) => {
  selectedDetail.value = null
  if (jobId) void loadJobDetail(jobId)
})

onMounted(() => {
  document.addEventListener('visibilitychange', handleVisibilityChange)
  void refreshPage()
})

onBeforeUnmount(() => {
  document.removeEventListener('visibilitychange', handleVisibilityChange)
  disposePolling()
  detailRequestSerial += 1
  detailController?.abort()
  detailController = null
})
</script>

<template>
  <UDashboardPanel :ui="{ body: 'p-0 sm:p-0' }">
    <template #header>
      <UDashboardNavbar title="格式转换">
        <template #right>
          <UBadge v-if="hasActiveJobs" color="info" variant="soft">
            <span class="mr-1 inline-block size-1.5 animate-pulse rounded-full bg-current" />
            正在同步进度
          </UBadge>
          <UButton
            icon="tabler:refresh"
            color="neutral"
            variant="ghost"
            :loading="isLoading"
            @click="refreshPage"
          >
            刷新
          </UButton>
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="dashboard-panel-body space-y-6">
        <DashboardPageHero
          eyebrow="转换任务"
          title="批量格式转换"
          description="选择一个或多个相簿后交给 Rust 后台并行处理。关闭页面不会停止任务，旧格式原图只会在你检查结果并手动确认后删除。"
          icon="tabler:arrows-exchange"
        >
          <template #actions>
            <UButton to="/dashboard/albums" color="neutral" variant="soft" icon="tabler:album">管理相簿</UButton>
          </template>
        </DashboardPageHero>

        <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <DashboardMetricCard label="进行中" :value="activeJobCount" icon="tabler:progress" tone="info" hint="异步任务" />
          <DashboardMetricCard label="已完成任务" :value="completedJobCount" icon="tabler:circle-check" tone="success" />
          <DashboardMetricCard label="成功转换图片" :value="convertedImageCount" icon="tabler:photo-check" tone="primary" />
          <DashboardMetricCard label="需要关注" :value="attentionJobCount" icon="tabler:alert-triangle" :tone="attentionJobCount ? 'warning' : 'neutral'" />
        </div>

        <UAlert
          v-if="pageError"
          color="error"
          variant="subtle"
          icon="tabler:alert-circle"
          title="转换页加载失败"
          :description="pageError"
        />

        <UAlert
          v-if="pollWarning"
          color="warning"
          variant="subtle"
          icon="tabler:wifi-off"
          title="进度轮询正在自动恢复"
          :description="pollWarning"
        />

        <div class="grid grid-cols-1 gap-6 xl:grid-cols-[minmax(300px,390px)_minmax(0,1fr)]">
          <div class="space-y-4">
            <UCard id="new-conversion" class="rounded-xl shadow-xs">
              <template #header>
                <div class="flex items-start gap-3">
                  <span class="flex size-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary"><Icon name="tabler:plus" class="size-5" /></span>
                  <div>
                    <h2 class="font-semibold text-highlighted">新建转换任务</h2>
                    <p class="mt-1 text-sm text-muted">选择目标格式和相簿范围</p>
                  </div>
                </div>
              </template>

              <div v-if="albums.length" class="space-y-5">
                <UFormField
                  label="目标格式"
                  description="JPG 与 JPEG 都生成标准 JPEG 图片。"
                  required
                >
                  <USelectMenu
                    v-model="targetFormat"
                    :items="formatOptions"
                    value-key="value"
                    label-key="label"
                    :search-input="false"
                    class="w-full"
                  />
                </UFormField>

                <div class="space-y-2">
                  <div class="flex items-center justify-between gap-3">
                    <label class="text-sm font-medium">选择相簿</label>
                    <UButton size="xs" color="neutral" variant="ghost" @click="toggleAllAlbums">
                      {{ allAlbumsSelected ? '清空' : '全选' }}
                    </UButton>
                  </div>

                  <div class="max-h-72 space-y-2 overflow-y-auto pr-1">
                    <button
                      v-for="album in albums"
                      :key="album.id"
                      type="button"
                      class="flex w-full items-center gap-3 rounded-xl border px-3 py-3 text-left transition"
                      :class="selectedAlbumIds.includes(album.id) ? 'border-primary/30 bg-primary/10' : 'border-default hover:bg-elevated'"
                      @click="toggleAlbum(album.id)"
                    >
                      <input
                        type="checkbox"
                        :checked="selectedAlbumIds.includes(album.id)"
                        class="size-4 rounded border-default text-primary"
                        @click.stop
                        @change="toggleAlbum(album.id)"
                      />
                      <span class="min-w-0 flex-1">
                        <span class="block truncate font-medium">{{ album.name }}</span>
                        <span class="mt-0.5 block text-xs text-muted">{{ album.photoCount }} 张图片</span>
                      </span>
                    </button>
                  </div>
                </div>

                <div class="rounded-xl bg-elevated px-3 py-3 text-sm text-muted"><p class="flex items-center gap-2 font-medium text-highlighted"><Icon name="tabler:filter" class="size-4 text-primary" />自动跳过已是目标格式的图片</p><p class="mt-1 text-xs">已选 {{ selectedAlbumIds.length }} 个相簿，只处理 PNG、JPG/JPEG、WEBP。</p></div>

                <UButton
                  block
                  icon="tabler:player-play"
                  :loading="isStarting"
                  :disabled="!selectedAlbumIds.length"
                  @click="startConversion"
                >
                  启动后台转换
                </UButton>
              </div>

              <div v-else-if="!isLoading" class="flex min-h-48 flex-col items-center justify-center text-center">
                <Icon name="tabler:album-off" class="size-9 text-muted" />
                <p class="mt-3 font-medium">没有可选相簿</p>
                <p class="mt-1 text-sm text-muted">请先创建相簿并上传图片。</p>
                <UButton class="mt-4" to="/dashboard/albums" variant="soft" icon="tabler:album">
                  前往相簿
                </UButton>
              </div>

              <div v-else class="space-y-3">
                <USkeleton v-for="index in 4" :key="index" class="h-14 w-full" />
              </div>
            </UCard>

            <UAlert
              color="warning"
              variant="subtle"
              icon="tabler:shield-check"
              title="旧格式图片永远不会自动删除"
              description="只有任务进入终态且新图验证通过后，管理员才能手动确认删除。"
            />
          </div>

          <div class="min-w-0 space-y-4">
            <UCard class="rounded-xl shadow-xs" :ui="{ body: 'p-2 sm:p-2' }">
              <template #header>
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <h2 class="font-semibold text-highlighted">任务队列</h2>
                    <p class="mt-1 text-sm text-muted">最近 100 个任务，选择后在下方查看详情</p>
                  </div>
                  <UBadge color="neutral" variant="soft">{{ jobs.length }}</UBadge>
                </div>
              </template>

              <div v-if="jobs.length" class="max-h-[420px] space-y-1 overflow-y-auto">
                <div
                  v-for="job in jobs"
                  :key="job.id"
                  class="flex items-center gap-2 rounded-xl border p-2 transition"
                  :class="selectedJobId === job.id ? 'border-primary/20 bg-primary/10' : 'border-transparent hover:bg-elevated'"
                >
                  <button type="button" class="min-w-0 flex-1 px-1 py-1 text-left" @click="selectJob(job.id)">
                    <div class="flex flex-wrap items-center gap-2">
                      <span class="font-medium">{{ job.targetFormat.toUpperCase() }}</span>
                      <UBadge :color="statusColor(job.status)" variant="soft" size="sm">
                        {{ statusLabel(job.status) }}
                      </UBadge>
                      <span class="font-mono text-xs text-muted">#{{ shortId(job.id) }}</span>
                    </div>
                    <div class="mt-2 flex items-center gap-3">
                      <UProgress :model-value="progressOf(job)" class="flex-1" />
                      <span class="w-20 text-right text-xs text-muted">{{ job.completed }}/{{ job.total }} · {{ progressOf(job) }}%</span>
                    </div>
                  </button>

                  <UButton
                    v-if="isActive(job)"
                    size="xs"
                    color="warning"
                    variant="soft"
                    icon="tabler:player-stop"
                    :loading="cancellingJobIds.has(job.id)"
                    :disabled="cancellingJobIds.has(job.id)"
                    @click="requestCancellation(job)"
                  >
                    {{ cancellingJobIds.has(job.id) ? '中断中' : '安全中断' }}
                  </UButton>
                </div>
              </div>

              <div v-else class="flex min-h-40 flex-col items-center justify-center text-center">
                <Icon name="tabler:history" class="size-9 text-muted" />
                <p class="mt-3 font-medium">还没有任务历史</p>
              </div>
            </UCard>

            <UCard v-if="detailJob" class="rounded-xl shadow-xs">
              <template #header>
                <div class="flex flex-wrap items-start justify-between gap-3">
                  <div class="min-w-0">
                    <div class="flex items-center gap-2">
                      <h2 class="font-semibold">任务详情 · {{ detailJob.targetFormat.toUpperCase() }}</h2>
                      <UBadge :color="statusColor(detailJob.status)" variant="soft">
                        {{ statusLabel(detailJob.status) }}
                      </UBadge>
                    </div>
                    <p class="mt-1 break-all font-mono text-xs text-muted">{{ detailJob.id }}</p>
                  </div>
                  <span class="text-xs text-muted">更新于 {{ formatTime(detailJob.updatedAt) }}</span>
                </div>
              </template>

              <div class="space-y-5">
                <div>
                  <div class="mb-2 flex justify-between text-sm">
                    <span>总进度</span>
                    <span>{{ detailJob.completed }} / {{ detailJob.total }}（{{ progressOf(detailJob) }}%）</span>
                  </div>
                  <UProgress
                    :model-value="progressOf(detailJob)"
                    :color="detailJob.failed > 0 ? 'warning' : detailJob.status === 'completed' ? 'success' : 'primary'"
                    class="w-full"
                  />
                </div>

                <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
                  <div class="rounded-md bg-elevated p-3">
                    <p class="text-xs text-muted">成功</p>
                    <p class="mt-1 text-lg font-semibold text-success">{{ detailJob.succeeded }}</p>
                  </div>
                  <div class="rounded-md bg-elevated p-3">
                    <p class="text-xs text-muted">失败</p>
                    <p class="mt-1 text-lg font-semibold text-error">{{ detailJob.failed }}</p>
                  </div>
                  <div class="rounded-md bg-elevated p-3">
                    <p class="text-xs text-muted">已取消</p>
                    <p class="mt-1 text-lg font-semibold">{{ detailJob.cancelled }}</p>
                  </div>
                  <div class="rounded-md bg-elevated p-3">
                    <p class="text-xs text-muted">待处理</p>
                    <p class="mt-1 text-lg font-semibold">{{ Math.max(0, detailJob.total - detailJob.completed) }}</p>
                  </div>
                </div>

                <UAlert
                  v-if="isActive(detailJob)"
                  color="info"
                  variant="subtle"
                  icon="tabler:loader-2"
                  title="服务端仍在处理"
                  description="为降低负载，执行期只轮询计数；详细条目会在终态后一次性拉取。"
                />

                <div v-if="isTerminal(detailJob)" class="rounded-lg border border-default p-4">
                  <div class="flex flex-wrap items-start justify-between gap-4">
                    <div>
                      <h3 class="font-medium">旧格式原图</h3>
                      <p v-if="detailJob.sourcesDeletedAt === null" class="mt-1 text-sm text-muted">
                        仍完整保留。请先检查转换成功数和下方条目，再决定是否删除。
                      </p>
                      <p
                        v-else-if="detailJob.sourcesDeletedAt !== null && detailJob.sourcesDeletedAt < 0"
                        class="mt-1 text-sm text-warning"
                      >
                        删除意图已持久化，后端正在处理或等待恢复未完成的项目。
                      </p>
                      <p v-else class="mt-1 text-sm text-success">
                        管理员已于 {{ formatTime(detailJob.sourcesDeletedAt) }} 确认删除旧图。
                      </p>
                    </div>
                    <UButton
                      v-if="detailJob.sourcesDeletedAt === null && detailJob.succeeded > 0"
                      color="error"
                      variant="soft"
                      icon="tabler:trash"
                      :loading="deletingJobId === detailJob.id"
                      @click="confirmSourceDeletion(detailJob)"
                    >
                      手动确认删除旧图
                    </UButton>
                  </div>

                  <UAlert
                    v-if="deletionResults[detailJob.id]?.failures.length"
                    class="mt-4"
                    color="warning"
                    variant="subtle"
                    icon="tabler:alert-triangle"
                    title="部分删除待后端恢复"
                    :description="`${deletionResults[detailJob.id]?.failures.length || 0} 个项目未完成，删除意图已写入持久化队列，不要重复提交。`"
                  />
                </div>

                <div v-if="isLoadingDetail" class="space-y-2">
                  <USkeleton v-for="index in 4" :key="index" class="h-12 w-full" />
                </div>

                <div v-else-if="selectedDetail?.items.length" class="space-y-2">
                  <div class="flex items-center justify-between gap-3">
                    <h3 class="font-medium">终态条目</h3>
                    <UBadge color="neutral" variant="soft">{{ selectedDetail.items.length }}</UBadge>
                  </div>
                  <div class="max-h-96 divide-y divide-default overflow-y-auto rounded-md border border-default px-3">
                    <div
                      v-for="item in selectedDetail.items"
                      :key="item.id"
                      class="flex items-start gap-3 py-3"
                    >
                      <Icon
                        :name="item.status === 'succeeded' ? 'tabler:circle-check' : item.status === 'failed' ? 'tabler:circle-x' : 'tabler:circle-minus'"
                        class="mt-0.5 size-5 shrink-0"
                        :class="item.status === 'succeeded' ? 'text-success' : item.status === 'failed' ? 'text-error' : 'text-muted'"
                      />
                      <div class="min-w-0 flex-1">
                        <div class="flex flex-wrap items-center gap-2">
                          <p class="break-all text-sm font-medium">{{ item.sourceName }}</p>
                          <UBadge :color="statusColor(item.status)" variant="soft" size="sm">
                            {{ statusLabel(item.status) }}
                          </UBadge>
                        </div>
                        <p v-if="item.error" class="mt-1 break-words text-xs text-error">{{ item.error }}</p>
                      </div>
                    </div>
                  </div>
                </div>

                <div v-else-if="isTerminal(detailJob)" class="rounded-md bg-elevated px-4 py-6 text-center text-sm text-muted">
                  该任务没有可显示的条目详情。
                </div>
              </div>
            </UCard>

            <UCard v-else-if="!isLoading">
              <div class="flex min-h-48 flex-col items-center justify-center text-center">
                <Icon name="tabler:list-details" class="size-9 text-muted" />
                <p class="mt-3 font-medium">选择一个任务查看详情</p>
              </div>
            </UCard>
          </div>
        </div>
      </div>
    </template>
  </UDashboardPanel>
</template>
