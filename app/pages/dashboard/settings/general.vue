<script lang="ts" setup>
import type { SiteSettings, SiteTheme } from '~/types/dashboard'

definePageMeta({
  layout: 'dashboard',
})

useHead({ title: '站点设置' })

const toast = useToast()
const colorMode = useColorMode()
const { adminFetch } = useAdminApi()
const { applySiteSettings } = useSiteSettings()

const themeOptions = [
  { label: '跟随系统', value: 'system', icon: 'tabler:device-desktop' },
  { label: '浅色', value: 'light', icon: 'tabler:sun' },
  { label: '深色', value: 'dark', icon: 'tabler:moon' },
]

const form = reactive<SiteSettings>({
  title: 'ChronoFrame',
  slogan: 'Frame the moments that matter.',
  author: 'ChronoFrame',
  avatarUrl: '/web-app-manifest-192x192.png',
  theme: 'system',
})
const savedSignature = ref('')
const isLoading = ref(false)
const isSaving = ref(false)
const loadError = ref('')
const avatarPreviewFailed = ref(false)

const formSignature = computed(() => JSON.stringify({
  title: form.title,
  slogan: form.slogan,
  author: form.author,
  avatarUrl: form.avatarUrl,
  theme: form.theme,
}))
const isDirty = computed(() => formSignature.value !== savedSignature.value)
const previewAvatarUrl = computed(() => form.avatarUrl.trim() || '/web-app-manifest-192x192.png')

const applyForm = (settings: SiteSettings) => {
  form.title = settings.title
  form.slogan = settings.slogan
  form.author = settings.author
  form.avatarUrl = settings.avatarUrl
  form.theme = settings.theme
  avatarPreviewFailed.value = false
  savedSignature.value = formSignature.value
}

const loadSettings = async () => {
  if (isLoading.value) return
  isLoading.value = true
  loadError.value = ''
  try {
    const settings = await adminFetch<SiteSettings>('/api/settings/site')
    applyForm(settings)
    applySiteSettings(settings)
  } catch (error) {
    loadError.value = getAdminApiErrorMessage(error)
  } finally {
    isLoading.value = false
  }
}

const resetForm = () => {
  void loadSettings()
}

const saveSettings = async () => {
  if (isSaving.value || !isDirty.value) return
  const title = form.title.trim()
  if (!title) {
    toast.add({ title: '网站名称不能为空', color: 'warning' })
    return
  }
  if (Array.from(title).length > 100 || Array.from(form.slogan.trim()).length > 200 || Array.from(form.author.trim()).length > 100) {
    toast.add({ title: '站点文字超出长度限制', color: 'warning' })
    return
  }
  const payload: SiteSettings = {
    title,
    slogan: form.slogan.trim(),
    author: form.author.trim(),
    avatarUrl: form.avatarUrl.trim(),
    theme: form.theme as SiteTheme,
  }

  isSaving.value = true
  try {
    const settings = await adminFetch<SiteSettings>('/api/settings/site', {
      method: 'PUT',
      body: payload,
    })
    applyForm(settings)
    applySiteSettings(settings)
    colorMode.preference = settings.theme
    toast.add({
      title: '站点设置已保存',
      description: '公开页面已立即使用新的名称、标语、作者、头像和默认主题。',
      color: 'success',
    })
  } catch (error) {
    toast.add({
      title: '保存站点设置失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isSaving.value = false
  }
}

const confirmDiscardChanges = () => !isDirty.value || window.confirm('站点设置尚未保存，确定要放弃修改吗？')
const handleBeforeUnload = (event: BeforeUnloadEvent) => {
  if (!isDirty.value) return
  event.preventDefault()
  event.returnValue = true
}

onBeforeRouteLeave(() => confirmDiscardChanges())
onMounted(() => {
  window.addEventListener('beforeunload', handleBeforeUnload)
  void loadSettings()
})
onBeforeUnmount(() => window.removeEventListener('beforeunload', handleBeforeUnload))
</script>

<template>
  <UDashboardPanel>
    <template #header>
      <UDashboardNavbar title="站点设置">
        <template #right>
          <UButton
            icon="tabler:refresh"
            color="neutral"
            variant="ghost"
            :loading="isLoading"
            :disabled="isSaving"
            @click="loadSettings"
          >
            刷新
          </UButton>
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="mx-auto w-full max-w-4xl space-y-6">
        <UAlert
          v-if="loadError"
          color="error"
          variant="subtle"
          icon="tabler:alert-circle"
          title="站点设置加载失败"
          :description="loadError"
        />

        <UCard>
          <template #header>
            <div>
              <h2 class="font-semibold">公开站点信息</h2>
              <p class="mt-1 text-sm text-muted">恢复原版的可自定义项目，配置保存在数据库中，不需要新增环境变量。</p>
            </div>
          </template>

          <form id="site-settings-form" class="space-y-5" @submit.prevent="saveSettings">
            <UFormField label="网站名称" description="显示在照片页、浏览器标题和管理后台。" required>
              <UInput v-model="form.title" maxlength="100" icon="tabler:world" class="w-full" :disabled="isLoading || isSaving" />
            </UFormField>

            <UFormField label="网站标语" description="显示在相簿首页和照片页；留空可隐藏。">
              <UInput v-model="form.slogan" maxlength="200" icon="tabler:quote" class="w-full" :disabled="isLoading || isSaving" />
            </UFormField>

            <UFormField label="作者名称" description="显示在照片页底部版权信息；留空时使用网站名称。">
              <UInput v-model="form.author" maxlength="100" icon="tabler:user" class="w-full" :disabled="isLoading || isSaving" />
            </UFormField>

            <UFormField label="头像 URL" description="支持以 / 开头的站内路径，或完整 HTTP(S) URL；留空使用默认图标。">
              <UInput v-model="form.avatarUrl" maxlength="2048" icon="tabler:photo" class="w-full" :disabled="isLoading || isSaving" @update:model-value="avatarPreviewFailed = false" />
            </UFormField>

            <div class="flex items-center gap-4 rounded-lg border border-default bg-elevated p-4">
              <img
                v-if="!avatarPreviewFailed"
                :src="previewAvatarUrl"
                alt="头像预览"
                class="size-16 rounded-full bg-default object-cover shadow-sm"
                @error="avatarPreviewFailed = true"
              >
              <span v-else class="flex size-16 items-center justify-center rounded-full bg-muted text-muted">
                <Icon name="tabler:photo-off" class="size-7" />
              </span>
              <div class="min-w-0">
                <p class="font-medium">头像预览</p>
                <p class="mt-1 truncate text-sm text-muted">{{ previewAvatarUrl }}</p>
              </div>
            </div>
          </form>
        </UCard>

        <UCard>
          <template #header>
            <div>
              <h2 class="font-semibold">默认外观</h2>
              <p class="mt-1 text-sm text-muted">访客首次打开网站时采用该主题，仍可在照片页临时切换。</p>
            </div>
          </template>
          <UFormField label="默认主题">
            <USelect
              v-model="form.theme"
              :items="themeOptions"
              value-key="value"
              label-key="label"
              icon="tabler:palette"
              class="w-full"
              :disabled="isLoading || isSaving"
            />
          </UFormField>
        </UCard>

        <UAlert
          v-if="isDirty"
          color="warning"
          variant="subtle"
          icon="tabler:edit"
          title="有未保存的更改"
          description="保存后公开页面会立即使用新设置。"
        />

        <div class="flex justify-end gap-2">
          <UButton color="neutral" variant="outline" :disabled="!isDirty || isLoading || isSaving" @click="resetForm">
            放弃修改
          </UButton>
          <UButton form="site-settings-form" type="submit" icon="tabler:device-floppy" :loading="isSaving" :disabled="!isDirty || isLoading">
            保存站点设置
          </UButton>
        </div>
      </div>
    </template>
  </UDashboardPanel>
</template>
