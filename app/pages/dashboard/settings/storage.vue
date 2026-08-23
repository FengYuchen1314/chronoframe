<script lang="ts" setup>
import type {
  StorageBackend,
  StorageSettings,
  StorageSettingsInput,
} from '~/types/dashboard'

definePageMeta({
  layout: 'dashboard',
})

useHead({ title: '存储设置' })

const toast = useToast()
const { adminFetch } = useAdminApi()

const backendOptions = [
  { label: '本地存储', value: 'local', icon: 'tabler:server' },
  { label: 'WebDAV', value: 'webdav', icon: 'tabler:cloud-upload' },
  { label: 'S3 对象存储', value: 's3', icon: 'tabler:brand-aws' },
]

const backendIcons: Record<StorageBackend, string> = {
  local: 'tabler:server',
  webdav: 'tabler:cloud-upload',
  s3: 'tabler:brand-aws',
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
const loadError = ref('')
const lastTest = ref<{ backend: StorageBackend, at: Date } | null>(null)

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
    applySettings(await adminFetch<StorageSettings>('/api/settings/storage'))
  } catch (error) {
    loadError.value = getAdminApiErrorMessage(error)
  } finally {
    isLoading.value = false
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
    storageTargetChanged.value
    && !window.confirm('确认更改活动存储目标？\n\n系统始终只使用一个活动后端。如果已有图片，后端会拒绝直接更改类型、路径、Endpoint、桶或前缀，避免产生不完整的存储引用。')
  ) return

  const payload = buildPayload()
  clearSensitiveInputs()
  isSaving.value = true

  try {
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

onMounted(loadSettings)
onBeforeUnmount(clearSensitiveInputs)
</script>

<template>
  <UDashboardPanel>
    <template #header>
      <UDashboardNavbar title="存储设置">
        <template #right>
          <UButton
            icon="tabler:refresh"
            color="neutral"
            variant="ghost"
            :loading="isLoading"
            @click="loadSettings"
          >
            重新读取
          </UButton>
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="mx-auto w-full max-w-5xl space-y-6">
        <section class="space-y-2 border-b border-default pb-4">
          <h1 class="text-xl font-semibold">存储设置</h1>
          <p class="text-sm text-muted">
            所有存储参数都在此后台设置并写入数据库，不使用环境变量配置。系统始终只有一个活动存储后端。
          </p>
        </section>

        <UAlert
          v-if="loadError"
          color="error"
          variant="subtle"
          icon="tabler:alert-circle"
          title="存储设置加载失败"
          :description="loadError"
        />

        <UAlert
          v-if="lastTest"
          color="success"
          variant="subtle"
          icon="tabler:circle-check"
          title="最近一次连接验证通过"
          :description="lastTestDescription"
        />

        <UCard>
          <template #header>
            <div class="flex flex-wrap items-start justify-between gap-3">
              <div>
                <h2 class="font-semibold">活动存储后端</h2>
                <p class="mt-1 text-sm text-muted">上传、缩略图、转换和旧图删除都使用该后端</p>
              </div>
              <UBadge color="success" variant="soft">
                <Icon :name="backendIcons[savedBackend]" class="mr-1 size-4" />
                已保存：{{ backendOptions.find(item => item.value === savedBackend)?.label }}
              </UBadge>
            </div>
          </template>

          <div v-if="isLoading" class="space-y-4">
            <USkeleton class="h-5 w-36" />
            <USkeleton class="h-10 w-full" />
            <USkeleton class="h-10 w-full" />
            <USkeleton class="h-10 w-full" />
          </div>

          <div v-else class="space-y-6">
            <UFormField
              label="存储类型"
              description="选择并保存后，它会成为唯一 active 后端。"
              required
            >
              <USelectMenu
                :model-value="form.backend"
                :items="backendOptions"
                value-key="value"
                label-key="label"
                :icon="backendIcons[form.backend]"
                :search-input="false"
                class="w-full sm:max-w-md"
                @update:model-value="changeBackend($event as StorageBackend)"
              />
            </UFormField>

            <USeparator />

            <section v-if="form.backend === 'local'" class="space-y-4">
              <div>
                <h3 class="font-semibold">本地存储</h3>
                <p class="mt-1 text-sm text-muted">路径由 Rust 服务进程读写；单文件 Docker 部署请保持 ./data/storage，确保图片位于可打包的持久化目录。</p>
              </div>
              <UFormField label="本地存储路径" required>
                <UInput
                  v-model="form.localPath"
                  icon="tabler:folder"
                  placeholder="./data/storage"
                  class="w-full"
                />
              </UFormField>
            </section>

            <section v-else-if="form.backend === 'webdav'" class="space-y-4">
              <div>
                <h3 class="font-semibold">WebDAV</h3>
                <p class="mt-1 text-sm text-muted">使用 HTTP(S) WebDAV 服务作为图片对象存储。</p>
              </div>

              <UFormField label="WebDAV 地址" description="必须是完整的 http:// 或 https:// URL。" required>
                <UInput
                  v-model="form.webdavUrl"
                  type="url"
                  icon="tabler:link"
                  placeholder="https://dav.example.com/remote.php/dav/files/user/"
                  class="w-full"
                />
              </UFormField>

              <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <UFormField label="用户名" required>
                  <UInput v-model="form.webdavUsername" autocomplete="username" icon="tabler:user" class="w-full" />
                </UFormField>
                <UFormField
                  label="密码"
                  :description="webdavPasswordSet ? '已安全保存；留空表示继续使用原密码。' : '首次启用 WebDAV 时必须输入。'"
                  :required="!webdavPasswordSet"
                >
                  <UInput
                    v-model="webdavPassword"
                    type="password"
                    autocomplete="new-password"
                    icon="tabler:key"
                    :placeholder="webdavPasswordSet ? '留空沿用已保存密码' : '输入 WebDAV 密码'"
                    class="w-full"
                  />
                </UFormField>
              </div>

              <UFormField label="存储前缀" description="不要以 / 开头；用于隔离 ChronoFrame 的对象。">
                <UInput v-model="form.webdavPrefix" icon="tabler:folder" placeholder="chronoframe" class="w-full" />
              </UFormField>
            </section>

            <section v-else class="space-y-4">
              <div>
                <h3 class="font-semibold">S3 对象存储</h3>
                <p class="mt-1 text-sm text-muted">支持 AWS S3 及兼容 S3 API 的自建对象存储。</p>
              </div>

              <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <UFormField label="S3 Endpoint" description="必须是完整的 HTTP(S) URL。" required>
                  <UInput v-model="form.s3Endpoint" type="url" icon="tabler:link" placeholder="https://s3.example.com" class="w-full" />
                </UFormField>
                <UFormField label="区域" required>
                  <UInput v-model="form.s3Region" icon="tabler:world" placeholder="us-east-1" class="w-full" />
                </UFormField>
                <UFormField label="桶名" required>
                  <UInput v-model="form.s3Bucket" icon="tabler:bucket" placeholder="chronoframe" class="w-full" />
                </UFormField>
                <UFormField label="存储前缀" description="不要以 / 开头。">
                  <UInput v-model="form.s3Prefix" icon="tabler:folder" placeholder="chronoframe" class="w-full" />
                </UFormField>
                <UFormField label="Access Key" required>
                  <UInput v-model="form.s3AccessKey" autocomplete="username" icon="tabler:id" class="w-full" />
                </UFormField>
                <UFormField
                  label="Secret Key"
                  :description="s3SecretKeySet ? '已安全保存；留空表示继续使用原密钥。' : '首次启用 S3 时必须输入。'"
                  :required="!s3SecretKeySet"
                >
                  <UInput
                    v-model="s3SecretKey"
                    type="password"
                    autocomplete="new-password"
                    icon="tabler:key"
                    :placeholder="s3SecretKeySet ? '留空沿用已保存密钥' : '输入 Secret Key'"
                    class="w-full"
                  />
                </UFormField>
              </div>
            </section>
          </div>

          <template #footer>
            <div class="space-y-3">
              <UAlert
                v-if="isDirty"
                color="warning"
                variant="subtle"
                icon="tabler:edit"
                title="有未保存的更改"
                description="「测试连接」不会保存；测试或保存请求发出后，密码和 Secret Key 输入框会立即清空。"
              />
              <div class="flex flex-wrap justify-end gap-2">
                <UButton
                  color="neutral"
                  variant="outline"
                  icon="tabler:plug-connected"
                  :loading="isTesting"
                  :disabled="isSaving"
                  @click="testConnection"
                >
                  测试连接
                </UButton>
                <UButton
                  icon="tabler:device-floppy"
                  :loading="isSaving"
                  :disabled="isTesting || !isDirty"
                  @click="saveSettings"
                >
                  保存并设为 active
                </UButton>
              </div>
            </div>
          </template>
        </UCard>

        <UAlert
          color="neutral"
          variant="subtle"
          icon="tabler:shield-lock"
          title="敏感字段不会回显"
          description="后端只返回「是否已设置」；页面不会回填 WebDAV 密码或 S3 Secret Key，也不会将它们写入浏览器存储或 URL。"
        />
      </div>
    </template>
  </UDashboardPanel>
</template>
