<script lang="ts" setup>
import { Alert as AAlert, Button as AButton, Card as ACard, Form as AForm, FormItem as AFormItem, Input as AInput, InputPassword as AInputPassword, RadioGroup as ARadioGroup, Space as ASpace, Tag as ATag, Progress as AProgress, Tabs as ATabs, TabPane as ATabPane, Descriptions as ADescriptions, DescriptionsItem as ADescriptionsItem } from 'ant-design-vue'
import type {
  Album,
  S3CleanupJob,
  StorageBackend,
  StorageMigrationJob,
  StorageSettings,
  StorageSettingsInput,
  ThumbnailRebuildJob,
} from '~/types/dashboard'

definePageMeta({
  layout: 'dashboard',
})

useHead({ title: '存储设置' })

const toast = useAdminNotice()
const { adminFetch } = useAdminApi()

const backendOptions: Array<{ label: string, value: StorageBackend, icon: string }> = [
  { label: '本地存储', value: 'local', icon: 'tabler:server' },
  { label: 'WebDAV', value: 'webdav', icon: 'tabler:cloud-upload' },
  { label: 'S3 对象存储', value: 's3', icon: 'tabler:brand-aws' },
]

const backendIcons: Record<StorageBackend, string> = {
  local: 'tabler:server',
  webdav: 'tabler:cloud-upload',
  s3: 'tabler:brand-aws',
}

const backendDescriptions: Record<StorageBackend, string> = {
  local: '随 Compose 数据目录一起备份迁移',
  webdav: '连接支持 WebDAV 的网盘或服务器',
  s3: '兼容 AWS S3、Cloudflare R2 等对象存储',
}

const form = reactive({
  backend: 'local' as StorageBackend,
  localPath: './data/storage',
  webdavUrl: '',
  webdavUsername: '',
  webdavPrefix: 'chronoframe',
  s3Endpoint: '',
  s3Region: 'us-east-1',
  s3Bucket: '',
  s3AccessKey: '',
  s3Prefix: 'chronoframe',
})

const webdavPassword = ref('')
const s3SecretKey = ref('')
const webdavPasswordSet = ref(false)
const s3SecretKeySet = ref(false)
const savedBackend = ref<StorageBackend>('local')
const savedSignature = ref('')
const savedTargetSignature = ref('')
const isLoading = ref(false)
const isTesting = ref(false)
const isSaving = ref(false)
const isLoadingMigrations = ref(false)
const isStorageTaskAction = ref(false)
const isLoadingThumbnailJob = ref(false)
const isThumbnailTaskAction = ref(false)
const isLoadingS3Cleanup = ref(false)
const isS3CleanupAction = ref(false)
const migrationJobs = ref<StorageMigrationJob[]>([])
const latestThumbnailJob = ref<ThumbnailRebuildJob | null>(null)
const latestS3Cleanup = ref<S3CleanupJob | null>(null)
const storedPhotoCount = ref(0)
const loadError = ref('')
const migrationLoadError = ref('')
const thumbnailLoadError = ref('')
const s3CleanupLoadError = ref('')
const lastTest = ref<{ backend: StorageBackend, at: Date } | null>(null)
let maintenancePoll: ReturnType<typeof setTimeout> | null = null
let pageMounted = false

const formSignature = computed(() => JSON.stringify({
  backend: form.backend,
  localPath: form.localPath,
  webdavUrl: form.webdavUrl,
  webdavUsername: form.webdavUsername,
  webdavPrefix: form.webdavPrefix,
  s3Endpoint: form.s3Endpoint,
  s3Region: form.s3Region,
  s3Bucket: form.s3Bucket,
  s3AccessKey: form.s3AccessKey,
  s3Prefix: form.s3Prefix,
}))

const isDirty = computed(() =>
  formSignature.value !== savedSignature.value
  || Boolean(webdavPassword.value)
  || Boolean(s3SecretKey.value),
)

const targetSignature = computed(() => {
  if (form.backend === 'local') {
    return JSON.stringify({ backend: form.backend, localPath: form.localPath.trim() })
  }
  if (form.backend === 'webdav') {
    return JSON.stringify({
      backend: form.backend,
      url: form.webdavUrl.trim(),
      prefix: form.webdavPrefix.trim(),
    })
  }
  return JSON.stringify({
    backend: form.backend,
    endpoint: form.s3Endpoint.trim(),
    region: form.s3Region.trim(),
    bucket: form.s3Bucket.trim(),
    prefix: form.s3Prefix.trim(),
  })
})

const storageTargetChanged = computed(() =>
  targetSignature.value !== savedTargetSignature.value,
)
const latestMigration = computed(() => migrationJobs.value[0] || null)
const activeStorageTask = computed(() => migrationJobs.value.find(job =>
  ['queued', 'running'].includes(job.status) || job.cleanupStatus === 'cleaning',
) || null)
const s3CleanupActive = computed(() => latestS3Cleanup.value?.status === 'running')
const storageBusy = computed(() => Boolean(activeStorageTask.value || s3CleanupActive.value))
const migrationRequired = computed(() => storageTargetChanged.value && storedPhotoCount.value > 0)
const migrationProgress = computed(() => {
  const job = latestMigration.value
  if (!job?.total) return 0
  return Math.min(100, Math.round((job.completed / job.total) * 100))
})
const thumbnailTaskActive = computed(() =>
  latestThumbnailJob.value && ['queued', 'running'].includes(latestThumbnailJob.value.status),
)
const thumbnailProgress = computed(() => {
  const job = latestThumbnailJob.value
  if (!job?.total) return job?.status === 'completed' ? 100 : 0
  return Math.min(100, Math.round((job.completed / job.total) * 100))
})
const s3CleanupProgress = computed(() => {
  const job = latestS3Cleanup.value
  if (!job?.total) return job?.status === 'completed' ? 100 : 0
  return Math.min(100, Math.round((job.completed / job.total) * 100))
})

const lastTestDescription = computed(() => {
  const result = lastTest.value
  if (!result) return ''
  const label = backendOptions.find(item => item.value === result.backend)?.label
  return `${label || result.backend} · ${result.at.toLocaleTimeString('zh-CN', { hour12: false })}`
})

const clearSensitiveInputs = () => {
  webdavPassword.value = ''
  s3SecretKey.value = ''
}

const applySettings = (settings: StorageSettings) => {
  form.backend = settings.backend
  form.localPath = settings.localPath || './data/storage'
  form.webdavUrl = settings.webdavUrl
  form.webdavUsername = settings.webdavUsername
  form.webdavPrefix = settings.webdavPrefix || 'chronoframe'
  form.s3Endpoint = settings.s3Endpoint
  form.s3Region = settings.s3Region || 'us-east-1'
  form.s3Bucket = settings.s3Bucket
  form.s3AccessKey = settings.s3AccessKey
  form.s3Prefix = settings.s3Prefix || 'chronoframe'
  webdavPasswordSet.value = settings.webdavPasswordSet
  s3SecretKeySet.value = settings.s3SecretKeySet
  savedBackend.value = settings.backend
  clearSensitiveInputs()
  savedSignature.value = formSignature.value
  savedTargetSignature.value = targetSignature.value
}

const buildPayload = (): StorageSettingsInput => ({
  backend: form.backend,
  localPath: form.localPath.trim(),
  webdavUrl: form.webdavUrl.trim(),
  webdavUsername: form.webdavUsername.trim(),
  webdavPassword: webdavPassword.value || undefined,
  webdavPrefix: form.webdavPrefix.trim(),
  s3Endpoint: form.s3Endpoint.trim(),
  s3Region: form.s3Region.trim(),
  s3Bucket: form.s3Bucket.trim(),
  s3AccessKey: form.s3AccessKey.trim(),
  s3SecretKey: s3SecretKey.value || undefined,
  s3Prefix: form.s3Prefix.trim(),
})

const loadSettings = async () => {
  if (isLoading.value) return
  isLoading.value = true
  loadError.value = ''
  lastTest.value = null

  try {
    const [settings, albums] = await Promise.all([
      adminFetch<StorageSettings>('/api/settings/storage'),
      adminFetch<Album[]>('/api/albums'),
    ])
    applySettings(settings)
    storedPhotoCount.value = albums.reduce((total, album) => total + album.photoCount, 0)
  } catch (error) {
    loadError.value = getAdminApiErrorMessage(error)
  } finally {
    isLoading.value = false
  }
}

const loadMigrations = async () => {
  if (isLoadingMigrations.value) return
  isLoadingMigrations.value = true
  try {
    migrationJobs.value = await adminFetch<StorageMigrationJob[]>('/api/storage-migrations')
    migrationLoadError.value = ''
  } catch (error) {
    migrationLoadError.value = getAdminApiErrorMessage(error)
  } finally {
    isLoadingMigrations.value = false
  }
}

const loadThumbnailJob = async () => {
  if (isLoadingThumbnailJob.value) return
  isLoadingThumbnailJob.value = true
  try {
    latestThumbnailJob.value = await adminFetch<ThumbnailRebuildJob | null>('/api/thumbnails/rebuilds/latest')
    thumbnailLoadError.value = ''
  } catch (error) {
    thumbnailLoadError.value = getAdminApiErrorMessage(error)
  } finally {
    isLoadingThumbnailJob.value = false
  }
}

const loadS3Cleanup = async () => {
  if (isLoadingS3Cleanup.value) return
  isLoadingS3Cleanup.value = true
  try {
    latestS3Cleanup.value = await adminFetch<S3CleanupJob | null>('/api/s3-cleanups/latest')
    s3CleanupLoadError.value = ''
  } catch (error) {
    s3CleanupLoadError.value = getAdminApiErrorMessage(error)
  } finally {
    isLoadingS3Cleanup.value = false
  }
}

const thumbnailStatusText = computed(() => {
  const job = latestThumbnailJob.value
  if (!job) return '尚未手动重建'
  if (job.status === 'running') return job.phase === 'clearing' ? '正在清空缓存' : '正在并发生成'
  return {
    queued: '等待开始',
    completed: '重建完成',
    failed: '部分生成失败',
    cancelled: '已安全中断',
    interrupted: '服务重启后待恢复',
  }[job.status] || job.status
})

const thumbnailStatusColor = computed((): 'default' | 'success' | 'error' | 'warning' | 'processing' => {
  const status = latestThumbnailJob.value?.status
  if (!status) return 'default'
  if (status === 'completed') return 'success'
  if (status === 'failed') return 'error'
  if (status === 'cancelled' || status === 'interrupted') return 'warning'
  return 'processing'
})

const runThumbnailTaskAction = async (action: 'start' | 'cancel' | 'resume') => {
  if (isThumbnailTaskAction.value) return
  isThumbnailTaskAction.value = true
  try {
    const job = latestThumbnailJob.value
    const endpoint = action === 'start'
      ? '/api/thumbnails/rebuilds'
      : `/api/thumbnails/rebuilds/${job?.id}/${action}`
    await adminFetch(endpoint, { method: 'POST' })
    toast.add({
      title: action === 'start' ? '三层派生图开始重建' : action === 'resume' ? '派生图重建已继续' : '已请求安全中断',
      description: action === 'cancel' ? '正在停止尚未开始的项目，已完成的派生图会保留。' : '任务在后端并发运行，可以离开此页面。每张图片会生成完整三层。',
      color: action === 'cancel' ? 'warning' : 'success',
    })
    await loadThumbnailJob()
  } catch (error) {
    toast.add({ title: '派生图任务操作失败', description: getAdminApiErrorMessage(error), color: 'error' })
  } finally {
    isThumbnailTaskAction.value = false
  }
}

const formatBytes = (bytes: number) => {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / 1024 ** unit
  return `${value >= 100 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`
}

const s3CleanupStatusText = computed(() => {
  const job = latestS3Cleanup.value
  if (!job) return '尚未扫描'
  if (job.status === 'running') return job.phase === 'scanning' ? '正在扫描对象' : '正在并发清理'
  if (job.status === 'ready') return job.total ? '等待确认清理' : '空间干净'
  return {
    completed: '清理完成',
    failed: '任务失败',
    cancelled: '已安全中断',
    interrupted: '服务重启后待继续',
  }[job.status] || job.status
})

const s3CleanupStatusColor = computed((): 'default' | 'success' | 'error' | 'warning' | 'processing' => {
  const status = latestS3Cleanup.value?.status
  if (!status) return 'default'
  if (status === 'completed' || (status === 'ready' && latestS3Cleanup.value?.total === 0)) return 'success'
  if (status === 'failed') return 'error'
  if (status === 'cancelled' || status === 'interrupted' || status === 'ready') return 'warning'
  return 'processing'
})

const runS3CleanupAction = async (action: 'scan' | 'delete' | 'cancel' | 'resume') => {
  if (isS3CleanupAction.value) return
  const job = latestS3Cleanup.value
  if (action === 'delete' && job && !await toast.confirm(`确定删除扫描到的 ${job.total} 个 S3 旧对象吗？\n\n预计释放 ${formatBytes(job.bytesFound)}。只会处理 ${job.managedPrefix}，删除前还会重新核对数据库引用；删除不能撤销。`)) return
  isS3CleanupAction.value = true
  try {
    const endpoint = action === 'scan'
      ? '/api/s3-cleanups/scan'
      : `/api/s3-cleanups/${job?.id}/${action}`
    await adminFetch(endpoint, { method: 'POST' })
    toast.add({
      title: action === 'scan' ? 'S3 旧空间扫描已开始' : action === 'delete' ? 'S3 旧对象开始清理' : action === 'resume' ? 'S3 任务已继续' : '已请求安全中断',
      description: action === 'delete' ? '任务在后端以 8 并发运行，可以离开此页面。' : action === 'scan' ? '只扫描 ChronoFrame 管理前缀；24 小时内的新对象不会进入清理清单。' : undefined,
      color: action === 'cancel' ? 'warning' : 'success',
    })
    await loadS3Cleanup()
  } catch (error) {
    toast.add({ title: 'S3 空间任务操作失败', description: getAdminApiErrorMessage(error), color: 'error' })
  } finally {
    isS3CleanupAction.value = false
  }
}

const migrationStatusText = (job: StorageMigrationJob) => {
  if (job.cleanupStatus === 'cleaning') return '正在清理旧存储'
  if (job.status === 'completed') {
    return {
      not_ready: '迁移完成',
      pending: '等待处理旧存储',
      cleaning: '正在清理旧存储',
      cleaned: '旧存储已清理',
      retained: '旧存储已保留',
      failed: '旧存储清理失败',
      interrupted: '旧存储清理已中断',
    }[job.cleanupStatus] || '迁移完成'
  }
  return {
    queued: '等待开始',
    running: '正在迁移',
    failed: '迁移失败',
    cancelled: '迁移已中断',
    interrupted: '迁移被重启中断',
  }[job.status] || job.status
}

const migrationStatusColor = (job: StorageMigrationJob): 'success' | 'error' | 'warning' | 'processing' => {
  if (job.cleanupStatus === 'cleaned' || job.cleanupStatus === 'retained') return 'success'
  if (job.status === 'failed' || job.cleanupStatus === 'failed') return 'error'
  if (job.status === 'cancelled' || job.status === 'interrupted' || job.cleanupStatus === 'interrupted') return 'warning'
  return 'processing'
}

const runStorageTaskAction = async (
  job: StorageMigrationJob,
  action: 'resume' | 'cancel' | 'cleanup' | 'retain',
) => {
  if (isStorageTaskAction.value) return
  if (action === 'cleanup' && !await toast.confirm(`确定删除迁移前 ${job.sourceBackend.toUpperCase()} 存储中的全部旧图片吗？\n\n系统会逐张校验当前存储中的副本后再删除，但删除动作不能撤销。`)) return
  if (action === 'retain' && !await toast.confirm('确定保留旧存储中的图片吗？\n\n系统会结束本次迁移流程，不会删除旧副本。')) return
  isStorageTaskAction.value = true
  try {
    await adminFetch(`/api/storage-migrations/${job.id}/${action}`, { method: 'POST' })
    toast.add({
      title: action === 'cleanup' ? '已开始清理旧存储' : action === 'retain' ? '已保留旧存储' : action === 'resume' ? '已继续迁移' : '已请求安全中断',
      color: action === 'cancel' ? 'warning' : 'success',
    })
    await loadMigrations()
  } catch (error) {
    toast.add({ title: '存储任务操作失败', description: getAdminApiErrorMessage(error), color: 'error' })
  } finally {
    isStorageTaskAction.value = false
  }
}

const testConnection = async () => {
  if (isTesting.value || isSaving.value) return
  const payload = buildPayload()
  clearSensitiveInputs()
  isTesting.value = true
  lastTest.value = null

  try {
    await adminFetch<{ ok: boolean }>('/api/settings/storage/test', {
      method: 'POST',
      body: payload,
    })
    lastTest.value = { backend: payload.backend, at: new Date() }
    toast.add({
      title: '存储连接测试通过',
      description: '本次测试不会保存配置。',
      color: 'success',
    })
  } catch (error) {
    toast.add({
      title: '存储连接测试失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    clearSensitiveInputs()
    isTesting.value = false
  }
}

const saveSettings = async () => {
  if (isTesting.value || isSaving.value) return
  if (
    migrationRequired.value
    && !await toast.confirm(`确认将 ${storedPhotoCount.value} 张图片迁移到新的存储位置？\n\n迁移会在后台复制并读回校验；完成后才切换存储。随后请在本页确认删除旧空间，或明确选择保留备份。`)
  ) return

  const payload = buildPayload()
  clearSensitiveInputs()
  isSaving.value = true

  try {
    if (migrationRequired.value) {
      await adminFetch<StorageMigrationJob>('/api/storage-migrations', { method: 'POST', body: payload })
      toast.add({
        title: '存储迁移已开始',
        description: '可以离开此页面；任务会持久化记录进度，服务重启后可手动继续。',
        color: 'success',
      })
      await loadMigrations()
    } else {
      const saved = await adminFetch<StorageSettings>('/api/settings/storage', {
        method: 'PUT',
        body: payload,
      })
      applySettings(saved)
      lastTest.value = { backend: saved.backend, at: new Date() }
      toast.add({
        title: '存储设置已保存',
        description: '后端已验证连接并将该配置设为唯一活动存储。',
        color: 'success',
      })
    }
  } catch (error) {
    toast.add({
      title: '存储设置保存失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    clearSensitiveInputs()
    isSaving.value = false
  }
}

const changeBackend = (backend: StorageBackend) => {
  clearSensitiveInputs()
  lastTest.value = null
  form.backend = backend
}

const pollMaintenance = async () => {
  if (!pageMounted) return
  const wasActive = storageBusy.value
  await Promise.all([loadMigrations(), loadThumbnailJob(), loadS3Cleanup()])
  if (wasActive && !storageBusy.value) await loadSettings()
  if (!pageMounted) return
  const interval = document.hidden ? 60_000 : storageBusy.value || thumbnailTaskActive.value ? 5_000 : 30_000
  maintenancePoll = window.setTimeout(pollMaintenance, interval)
}

onMounted(async () => {
  pageMounted = true
  await loadSettings()
  await Promise.all([loadMigrations(), loadThumbnailJob(), loadS3Cleanup()])
  maintenancePoll = window.setTimeout(pollMaintenance, storageBusy.value || thumbnailTaskActive.value ? 5_000 : 30_000)
})
onBeforeUnmount(() => {
  pageMounted = false
  clearSensitiveInputs()
  if (maintenancePoll !== null) window.clearTimeout(maintenancePoll)
})
const storageTab = ref('connection')
onBeforeRouteLeave(() => !isDirty.value || toast.confirm('存储设置尚未保存，确定放弃修改并离开吗？'))
</script>

<template>
  <div>
    <DashboardPageHeader title="存储与维护" description="原图存储配置保存在数据库中；迁移和清理任务在后台运行。"><ATag color="success">{{ backendOptions.find(item => item.value === savedBackend)?.label }}</ATag><AButton :loading="isLoading" @click="loadSettings(); loadMigrations(); loadThumbnailJob(); loadS3Cleanup()">刷新</AButton></DashboardPageHeader>
    <AAlert v-if="loadError" type="error" show-icon :message="loadError" class="mb-5" />
    <ATabs v-model:active-key="storageTab">
      <ATabPane key="connection" tab="存储连接">
        <ACard title="原图存储配置" style="max-width:1060px">
          <AAlert v-if="lastTest" type="success" show-icon message="连接测试通过" :description="lastTestDescription" class="mb-5" />
          <AForm layout="vertical" :model="form" @finish="saveSettings">
            <AFormItem label="存储类型" name="backend"><ARadioGroup :value="form.backend" :options="backendOptions" :disabled="isLoading || isSaving || isTesting || storageBusy" @change="changeBackend($event.target.value)" /></AFormItem>
            <template v-if="form.backend === 'local'"><AFormItem label="本地存储路径" name="localPath" extra="容器内路径，建议保持在 /app/data 下，随数据目录持久化。"><AInput v-model:value="form.localPath" /></AFormItem></template>
            <div v-else-if="form.backend === 'webdav'" class="admin-form-grid">
              <AFormItem label="WebDAV URL" name="webdavUrl" required><AInput v-model:value="form.webdavUrl" placeholder="https://dav.example.com" /></AFormItem>
              <AFormItem label="目录前缀" name="webdavPrefix"><AInput v-model:value="form.webdavPrefix" /></AFormItem>
              <AFormItem label="用户名" name="webdavUsername"><AInput v-model:value="form.webdavUsername" autocomplete="off" /></AFormItem>
              <AFormItem label="密码" :extra="webdavPasswordSet ? '已配置，留空保持不变。' : '首次使用请填写。'"><AInputPassword v-model:value="webdavPassword" autocomplete="new-password" /></AFormItem>
            </div>
            <div v-else class="admin-form-grid">
              <AFormItem label="S3 Endpoint" name="s3Endpoint" required extra="R2 使用账户的 S3 API 地址，不包含桶名。"><AInput v-model:value="form.s3Endpoint" placeholder="https://account-id.r2.cloudflarestorage.com" /></AFormItem>
              <AFormItem label="区域" name="s3Region" required extra="Cloudflare R2 填 auto"><AInput v-model:value="form.s3Region" /></AFormItem>
              <AFormItem label="桶名" name="s3Bucket" required><AInput v-model:value="form.s3Bucket" /></AFormItem>
              <AFormItem label="存储前缀" name="s3Prefix" extra="不要以 / 开头"><AInput v-model:value="form.s3Prefix" /></AFormItem>
              <AFormItem label="Access Key" name="s3AccessKey" required><AInput v-model:value="form.s3AccessKey" autocomplete="off" /></AFormItem>
              <AFormItem label="Secret Key" :extra="s3SecretKeySet ? '已配置，留空保持不变。' : '首次使用请填写。'"><AInputPassword v-model:value="s3SecretKey" autocomplete="new-password" /></AFormItem>
            </div>
            <AAlert v-if="storageBusy" type="warning" show-icon message="存储任务正在运行，请完成或中断任务后再修改连接。" class="mb-5" />
            <AAlert v-else-if="migrationRequired" type="info" show-icon :message="'已存在 ' + storedPhotoCount + ' 张图片，将先复制校验，再切换到新存储。'" class="mb-5" />
            <ASpace wrap><AButton type="primary" html-type="submit" :loading="isSaving" :disabled="isTesting || !isDirty || storageBusy">{{ migrationRequired ? '开始安全迁移' : '保存并启用' }}</AButton><AButton :loading="isTesting" :disabled="isSaving || storageBusy" @click="testConnection">测试连接</AButton><AButton :disabled="!isDirty || isSaving" @click="loadSettings">重置</AButton></ASpace>
            <p class="admin-help mt-4">测试不会保存配置。密码和 Secret Key 发送后立即清空，不会写入浏览器存储。相册 ZIP 与此处配置无关，始终保存在本地。</p>
          </AForm>
        </ACard>
      </ATabPane>
      <ATabPane key="migration" tab="存储迁移">
        <AAlert v-if="migrationLoadError" type="warning" :message="migrationLoadError" show-icon class="mb-5" />
        <AAlert v-if="!latestMigration" type="info" message="暂无迁移记录。修改存储连接并保存后，会自动创建迁移任务。" show-icon />
        <ACard v-else title="最近一次迁移">
          <template #extra><ATag :color="migrationStatusColor(latestMigration)">{{ migrationStatusText(latestMigration) }}</ATag></template>
          <ADescriptions :column="3"><ADescriptionsItem label="来源">{{ latestMigration.sourceBackend.toUpperCase() }}</ADescriptionsItem><ADescriptionsItem label="目标">{{ latestMigration.targetBackend.toUpperCase() }}</ADescriptionsItem><ADescriptionsItem label="图片">{{ latestMigration.total }}</ADescriptionsItem></ADescriptions>
          <AProgress :percent="latestMigration.cleanupStatus === 'cleaning' ? (latestMigration.total ? Math.round(latestMigration.cleanupCompleted / latestMigration.total * 100) : 0) : migrationProgress" />
          <p class="admin-help mt-2 mb-4">成功 {{ latestMigration.succeeded }} · 失败 {{ latestMigration.failed }} · 旧对象已清理 {{ latestMigration.cleanupCompleted }}</p>
          <AAlert v-if="latestMigration.error" type="warning" :message="latestMigration.error" class="mb-4" />
          <AAlert v-if="latestMigration.status === 'completed' && ['pending','failed','interrupted'].includes(latestMigration.cleanupStatus)" type="warning" show-icon message="新存储已启用，请决定是否删除旧存储中的副本。" class="mb-4" />
          <ASpace wrap>
            <AButton v-if="['queued','running'].includes(latestMigration.status) || latestMigration.cleanupStatus === 'cleaning'" :loading="isStorageTaskAction" @click="runStorageTaskAction(latestMigration, 'cancel')">安全中断</AButton>
            <AButton v-if="['failed','cancelled','interrupted'].includes(latestMigration.status)" type="primary" :loading="isStorageTaskAction" @click="runStorageTaskAction(latestMigration, 'resume')">继续迁移</AButton>
            <template v-if="latestMigration.status === 'completed' && ['pending','failed','interrupted'].includes(latestMigration.cleanupStatus)"><AButton danger :loading="isStorageTaskAction" @click="runStorageTaskAction(latestMigration, 'cleanup')">删除旧存储图片</AButton><AButton :loading="isStorageTaskAction" @click="runStorageTaskAction(latestMigration, 'retain')">保留旧副本</AButton></template>
          </ASpace>
        </ACard>
      </ATabPane>
      <ATabPane key="cache" tab="图片缓存">
        <ACard title="重建三层浏览图">
          <template #extra><ATag :color="thumbnailStatusColor">{{ thumbnailStatusText }}</ATag></template>
          <p class="admin-help mb-5">生成 320px PNG、≤1.5 MB WebP 预览和 ≤5 MB WebP 高清图。此操作不改变原图，也不删除下载 ZIP。</p>
          <AAlert v-if="thumbnailLoadError" type="warning" :message="thumbnailLoadError" class="mb-4" />
          <template v-if="latestThumbnailJob"><AProgress :percent="thumbnailProgress" /><p class="admin-help mt-2 mb-5">{{ latestThumbnailJob.completed }} / {{ latestThumbnailJob.total }} · 成功 {{ latestThumbnailJob.succeeded }} · 失败 {{ latestThumbnailJob.failed }} · 并发 {{ latestThumbnailJob.workerCount }}</p><AAlert v-if="latestThumbnailJob.error" type="warning" :message="latestThumbnailJob.error" class="mb-4" /></template>
          <ASpace><AButton v-if="thumbnailTaskActive" :loading="isThumbnailTaskAction" @click="runThumbnailTaskAction('cancel')">安全中断</AButton><AButton v-else type="primary" :loading="isThumbnailTaskAction" :disabled="storageBusy" @click="runThumbnailTaskAction('start')">清空并重新生成</AButton><AButton v-if="latestThumbnailJob && ['failed','cancelled','interrupted'].includes(latestThumbnailJob.status)" :loading="isThumbnailTaskAction" @click="runThumbnailTaskAction('resume')">继续上次任务</AButton></ASpace>
        </ACard>
      </ATabPane>
      <ATabPane key="cleanup" tab="S3 空间清理">
        <ACard title="清理失去引用的旧对象">
          <template #extra><ATag :color="s3CleanupStatusColor">{{ s3CleanupStatusText }}</ATag></template>
          <AAlert type="info" show-icon message="先扫描，再由管理员确认删除" description="仅处理 ChronoFrame 管理前缀，保护数据库引用和 24 小时内的新对象。不会删除本地 ZIP。" class="mb-5" />
          <AAlert v-if="s3CleanupLoadError" type="warning" :message="s3CleanupLoadError" class="mb-4" />
          <template v-if="latestS3Cleanup">
            <ADescriptions :column="3"><ADescriptionsItem label="已扫描对象">{{ latestS3Cleanup.scannedObjects }}</ADescriptionsItem><ADescriptionsItem label="候选旧对象">{{ latestS3Cleanup.total }}</ADescriptionsItem><ADescriptionsItem label="预计释放">{{ formatBytes(latestS3Cleanup.bytesFound) }}</ADescriptionsItem></ADescriptions>
            <AProgress :percent="s3CleanupProgress" /><p class="admin-help mt-2 mb-5">已删除 {{ latestS3Cleanup.deleted }} · 已释放 {{ formatBytes(latestS3Cleanup.bytesDeleted) }} · 失败 {{ latestS3Cleanup.failed }}</p>
            <AAlert v-if="latestS3Cleanup.error" type="warning" :message="latestS3Cleanup.error" class="mb-4" />
          </template>
          <ASpace wrap><AButton v-if="s3CleanupActive" :loading="isS3CleanupAction" @click="runS3CleanupAction('cancel')">安全中断</AButton><AButton v-else type="primary" :disabled="savedBackend !== 's3' || storageBusy" :loading="isS3CleanupAction" @click="runS3CleanupAction('scan')">扫描旧对象</AButton><AButton v-if="latestS3Cleanup?.status === 'ready' && latestS3Cleanup.total > 0" danger :loading="isS3CleanupAction" @click="runS3CleanupAction('delete')">确认删除旧对象</AButton><AButton v-if="latestS3Cleanup && ['failed','cancelled','interrupted'].includes(latestS3Cleanup.status)" :loading="isS3CleanupAction" @click="runS3CleanupAction('resume')">继续任务</AButton></ASpace>
        </ACard>
      </ATabPane>
    </ATabs>
  </div>
</template>
