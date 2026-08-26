<script lang="ts" setup>
import type {
  Album,
  StorageBackend,
  StorageMigrationJob,
  StorageSettings,
  StorageSettingsInput,
} from '~/types/dashboard'

definePageMeta({
  layout: 'dashboard',
})

useHead({ title: '存储设置' })

const toast = useToast()
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
const migrationJobs = ref<StorageMigrationJob[]>([])
const storedPhotoCount = ref(0)
const loadError = ref('')
const lastTest = ref<{ backend: StorageBackend, at: Date } | null>(null)
let migrationPoll: ReturnType<typeof setInterval> | null = null

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
const migrationRequired = computed(() => storageTargetChanged.value && storedPhotoCount.value > 0)
const migrationProgress = computed(() => {
  const job = latestMigration.value
  if (!job?.total) return 0
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
  } catch (error) {
    if (!loadError.value) loadError.value = getAdminApiErrorMessage(error)
  } finally {
    isLoadingMigrations.value = false
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

const migrationStatusColor = (job: StorageMigrationJob): 'success' | 'error' | 'warning' | 'primary' => {
  if (job.cleanupStatus === 'cleaned' || job.cleanupStatus === 'retained') return 'success'
  if (job.status === 'failed' || job.cleanupStatus === 'failed') return 'error'
  if (job.status === 'cancelled' || job.status === 'interrupted' || job.cleanupStatus === 'interrupted') return 'warning'
  return 'primary'
}

const runStorageTaskAction = async (
  job: StorageMigrationJob,
  action: 'resume' | 'cancel' | 'cleanup' | 'retain',
) => {
  if (isStorageTaskAction.value) return
  if (action === 'cleanup' && !window.confirm(`确定删除迁移前 ${job.sourceBackend.toUpperCase()} 存储中的全部旧图片吗？\n\n系统会逐张校验当前存储中的副本后再删除，但删除动作不能撤销。`)) return
  if (action === 'retain' && !window.confirm('确定保留旧存储中的图片吗？\n\n系统会结束本次迁移流程，不会删除旧副本。')) return
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
    && !window.confirm(`确认将 ${storedPhotoCount.value} 张图片迁移到新的存储位置？\n\n迁移会在后台复制并读回校验；完成后才切换存储。旧存储不会自动删除。`)
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

onMounted(async () => {
  await Promise.all([loadSettings(), loadMigrations()])
  migrationPoll = window.setInterval(async () => {
    const wasActive = Boolean(activeStorageTask.value)
    await loadMigrations()
    if (wasActive && !activeStorageTask.value) await loadSettings()
  }, 2000)
})
onBeforeUnmount(() => {
  clearSensitiveInputs()
  if (migrationPoll !== null) window.clearInterval(migrationPoll)
})
</script>

<template>
  <UDashboardPanel :ui="{ body: 'p-0 sm:p-0' }">
    <template #header>
      <UDashboardNavbar title="存储设置">
        <template #right><UButton icon="tabler:refresh" color="neutral" variant="ghost" :loading="isLoading || isLoadingMigrations" @click="loadSettings(); loadMigrations()">重新读取</UButton></template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="dashboard-panel-body space-y-6">
        <DashboardPageHero eyebrow="存储设置" title="图片存储与迁移" description="配置本地、WebDAV 或 S3，并在同一页测试连接、切换后端和查看迁移进度。所有配置均保存在后台数据库。" icon="tabler:database-cog">
          <template #actions><UBadge color="success" variant="soft"><Icon :name="backendIcons[savedBackend]" class="mr-1 size-4" />当前：{{ backendOptions.find(item => item.value === savedBackend)?.label }}</UBadge></template>
        </DashboardPageHero>

        <UAlert v-if="loadError" color="error" variant="subtle" icon="tabler:alert-circle" title="存储设置加载失败" :description="loadError" />
        <UAlert v-if="lastTest" color="success" variant="subtle" icon="tabler:circle-check" title="最近一次连接验证通过" :description="lastTestDescription" />

        <section v-if="latestMigration" class="dashboard-section overflow-hidden">
          <header class="flex flex-wrap items-start justify-between gap-3 border-b border-default px-5 py-4 sm:px-6">
            <div class="flex items-start gap-3"><span class="flex size-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary"><Icon name="tabler:transfer" class="size-5" /></span><div><h2 class="font-semibold text-highlighted">存储迁移</h2><p class="mt-1 text-sm text-muted">{{ latestMigration.sourceBackend.toUpperCase() }} → {{ latestMigration.targetBackend.toUpperCase() }} · {{ latestMigration.total }} 张图片</p></div></div>
            <UBadge :color="migrationStatusColor(latestMigration)" variant="soft">{{ migrationStatusText(latestMigration) }}</UBadge>
          </header>
          <div class="space-y-4 p-5 sm:p-6">
            <div v-if="latestMigration.cleanupStatus === 'cleaning' || ['cleaned', 'retained'].includes(latestMigration.cleanupStatus)">
              <div class="mb-2 flex items-center justify-between text-sm"><span class="text-muted">旧存储处理进度</span><strong class="text-highlighted">{{ latestMigration.cleanupCompleted }} / {{ latestMigration.total }}</strong></div>
              <UProgress :model-value="latestMigration.total ? Math.round(latestMigration.cleanupCompleted / latestMigration.total * 100) : 0" />
            </div>
            <div v-else>
              <div class="mb-2 flex items-center justify-between text-sm"><span class="text-muted">复制并校验进度</span><strong class="text-highlighted">{{ latestMigration.completed }} / {{ latestMigration.total }}（{{ migrationProgress }}%）</strong></div>
              <UProgress :model-value="migrationProgress" />
              <p class="mt-2 text-xs text-muted">成功 {{ latestMigration.succeeded }} · 失败 {{ latestMigration.failed }} · 已中断 {{ latestMigration.cancelled }}</p>
            </div>
            <UAlert v-if="latestMigration.error" color="warning" variant="subtle" icon="tabler:alert-triangle" title="任务提示" :description="latestMigration.error" />
            <div v-if="latestMigration.status === 'completed' && ['pending', 'failed', 'interrupted'].includes(latestMigration.cleanupStatus)" class="rounded-xl border border-warning/20 bg-warning/10 p-4">
              <p class="font-medium text-warning">新存储已启用，旧存储尚未处理</p>
              <p class="mt-1 text-xs leading-5 text-muted">删除前会再次读回并校验新存储中的每张图片。也可以选择保留旧副本作为备份。</p>
            </div>
            <div class="flex flex-wrap justify-end gap-2">
              <UButton v-if="['queued', 'running'].includes(latestMigration.status) || latestMigration.cleanupStatus === 'cleaning'" color="warning" variant="soft" icon="tabler:player-stop" :loading="isStorageTaskAction" @click="runStorageTaskAction(latestMigration, 'cancel')">安全中断</UButton>
              <UButton v-if="['failed', 'cancelled', 'interrupted'].includes(latestMigration.status)" icon="tabler:player-play" :loading="isStorageTaskAction" @click="runStorageTaskAction(latestMigration, 'resume')">继续迁移</UButton>
              <template v-if="latestMigration.status === 'completed' && ['pending', 'failed', 'interrupted'].includes(latestMigration.cleanupStatus)">
                <UButton color="neutral" variant="soft" icon="tabler:archive" :loading="isStorageTaskAction" @click="runStorageTaskAction(latestMigration, 'retain')">保留旧存储</UButton>
                <UButton color="error" icon="tabler:trash" :loading="isStorageTaskAction" @click="runStorageTaskAction(latestMigration, 'cleanup')">删除旧存储图片</UButton>
              </template>
            </div>
          </div>
        </section>

        <div class="grid items-start gap-5 xl:grid-cols-[300px_minmax(0,1fr)]">
          <aside class="space-y-4 xl:sticky xl:top-4">
            <section class="dashboard-section overflow-hidden">
              <div class="border-b border-default px-4 py-4"><h2 class="font-semibold text-highlighted">1. 选择存储类型</h2><p class="mt-1 text-sm text-muted">选择后只填写对应配置</p></div>
              <div class="space-y-2 p-2">
                <button v-for="option in backendOptions" :key="option.value" type="button" class="flex w-full items-center gap-3 rounded-xl border p-3 text-left transition" :class="form.backend === option.value ? 'border-primary/30 bg-primary/10' : 'border-transparent hover:bg-elevated'" :disabled="isLoading || isTesting || isSaving || Boolean(activeStorageTask)" @click="changeBackend(option.value)">
                  <span class="flex size-10 shrink-0 items-center justify-center rounded-xl" :class="form.backend === option.value ? 'bg-primary text-inverted' : 'bg-elevated text-muted'"><Icon :name="option.icon" class="size-5" /></span>
                  <span class="min-w-0 flex-1"><span class="block text-sm font-medium text-highlighted">{{ option.label }}</span><span class="mt-0.5 block text-xs leading-5 text-muted">{{ backendDescriptions[option.value] }}</span></span>
                  <Icon v-if="form.backend === option.value" name="tabler:check" class="size-4 shrink-0 text-primary" />
                </button>
              </div>
            </section>

            <section class="dashboard-section p-4">
              <div class="flex items-start gap-3"><span class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-success/10 text-success"><Icon name="tabler:shield-lock" class="size-5" /></span><div><h3 class="text-sm font-semibold text-highlighted">密钥不会回显</h3><p class="mt-1 text-xs leading-5 text-muted">后端只返回是否已设置。密码和 Secret Key 不会写入浏览器存储或 URL。</p></div></div>
            </section>

            <section v-if="storageTargetChanged" class="rounded-xl border border-warning/20 bg-warning/10 p-4 text-sm text-warning">
              <p class="flex items-center gap-2 font-medium"><Icon name="tabler:alert-triangle" class="size-4" />存储目标发生变化</p><p class="mt-2 text-xs leading-5">{{ storedPhotoCount ? `保存后会迁移 ${storedPhotoCount} 张图片，全部校验成功才切换。` : '当前没有图片，可以直接切换。' }}</p>
            </section>
          </aside>

          <section class="dashboard-section overflow-hidden">
            <header class="flex flex-wrap items-start justify-between gap-3 border-b border-default px-5 py-4 sm:px-6">
              <div class="flex items-start gap-3"><span class="flex size-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary"><Icon :name="backendIcons[form.backend]" class="size-5" /></span><div><h2 class="font-semibold text-highlighted">2. 配置 {{ backendOptions.find(item => item.value === form.backend)?.label }}</h2><p class="mt-1 text-sm text-muted">上传、缩略图、转换和旧图删除都会使用这里的配置</p></div></div>
              <UBadge v-if="form.backend === savedBackend" color="success" variant="soft">当前已启用</UBadge>
            </header>

            <div v-if="isLoading" class="space-y-4 p-6"><USkeleton class="h-5 w-36" /><USkeleton class="h-11 w-full" /><USkeleton class="h-11 w-full" /><USkeleton class="h-11 w-full" /></div>

            <div v-else class="p-5 sm:p-6">
              <section v-if="form.backend === 'local'" class="space-y-5">
                <div class="rounded-xl bg-elevated p-4 text-sm leading-6 text-muted"><p class="font-medium text-highlighted">适合单机 Compose 部署</p><p class="mt-1">建议保持 <code class="rounded bg-default px-1.5 py-0.5 text-xs">./data/storage</code>，图片会和数据库一起位于当前部署目录的持久化数据中。</p></div>
                <UFormField label="本地存储路径" description="该路径由 Rust 服务进程读写。" required><UInput v-model="form.localPath" icon="tabler:folder" size="lg" placeholder="./data/storage" class="w-full" /></UFormField>
              </section>

              <section v-else-if="form.backend === 'webdav'" class="space-y-5">
                <UFormField label="WebDAV 地址" description="必须是完整的 http:// 或 https:// URL。" required><UInput v-model="form.webdavUrl" type="url" icon="tabler:link" size="lg" placeholder="https://dav.example.com/remote.php/dav/files/user/" class="w-full" /></UFormField>
                <div class="grid gap-5 sm:grid-cols-2">
                  <UFormField label="用户名" required><UInput v-model="form.webdavUsername" autocomplete="username" icon="tabler:user" class="w-full" /></UFormField>
                  <UFormField label="密码" :description="webdavPasswordSet ? '已安全保存；留空继续使用原密码。' : '首次启用时必须输入。'" :required="!webdavPasswordSet"><UInput v-model="webdavPassword" type="password" autocomplete="new-password" icon="tabler:key" :placeholder="webdavPasswordSet ? '留空沿用已保存密码' : '输入 WebDAV 密码'" class="w-full" /></UFormField>
                </div>
                <UFormField label="存储前缀" description="不要以 / 开头，用于隔离 ChronoFrame 对象。"><UInput v-model="form.webdavPrefix" icon="tabler:folder" placeholder="chronoframe" class="w-full" /></UFormField>
              </section>

              <section v-else class="space-y-5">
                <div class="rounded-xl border border-info/20 bg-info/10 p-4 text-sm leading-6 text-info"><p class="font-medium">Cloudflare R2 提示</p><p class="mt-1 text-xs">Endpoint 只填写到账户级 <code>/</code> 根地址，不要把桶名附在 URL 后；R2 区域通常填写 <code>auto</code>。</p></div>
                <div class="grid gap-5 sm:grid-cols-2">
                  <UFormField label="S3 Endpoint" description="完整 HTTP(S) URL，不包含桶名。" required><UInput v-model="form.s3Endpoint" type="url" icon="tabler:link" placeholder="https://account-id.r2.cloudflarestorage.com" class="w-full" /></UFormField>
                  <UFormField label="区域" required><UInput v-model="form.s3Region" icon="tabler:world" placeholder="auto 或 us-east-1" class="w-full" /></UFormField>
                  <UFormField label="桶名" required><UInput v-model="form.s3Bucket" icon="tabler:bucket" placeholder="chronoframe" class="w-full" /></UFormField>
                  <UFormField label="存储前缀" description="不要以 / 开头。"><UInput v-model="form.s3Prefix" icon="tabler:folder" placeholder="chronoframe" class="w-full" /></UFormField>
                  <UFormField label="Access Key" required><UInput v-model="form.s3AccessKey" autocomplete="username" icon="tabler:id" class="w-full" /></UFormField>
                  <UFormField label="Secret Key" :description="s3SecretKeySet ? '已安全保存；留空继续使用原密钥。' : '首次启用时必须输入。'" :required="!s3SecretKeySet"><UInput v-model="s3SecretKey" type="password" autocomplete="new-password" icon="tabler:key" :placeholder="s3SecretKeySet ? '留空沿用已保存密钥' : '输入 Secret Key'" class="w-full" /></UFormField>
                </div>
              </section>
            </div>

            <footer class="border-t border-default bg-muted/50 p-4 sm:px-6">
              <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <p class="flex items-center gap-2 text-sm" :class="isDirty ? 'text-warning' : 'text-muted'"><Icon :name="isDirty ? 'tabler:edit-circle' : 'tabler:circle-check'" class="size-5" />{{ isDirty ? '配置有未保存的更改' : '当前配置已保存' }}</p>
                <div class="flex flex-wrap justify-end gap-2"><UButton color="neutral" variant="outline" icon="tabler:plug-connected" :loading="isTesting" :disabled="isSaving || Boolean(activeStorageTask)" @click="testConnection">只测试连接</UButton><UButton :icon="migrationRequired ? 'tabler:transfer' : 'tabler:device-floppy'" :loading="isSaving" :disabled="isTesting || !isDirty || Boolean(activeStorageTask)" @click="saveSettings">{{ migrationRequired ? '开始安全迁移' : '验证、保存并启用' }}</UButton></div>
              </div>
              <p class="mt-3 text-xs leading-5 text-muted">测试或保存请求发出后，密码和 Secret Key 输入框会立即清空。</p>
            </footer>
          </section>
        </div>
      </div>
    </template>
  </UDashboardPanel>
</template>
