<script lang="ts" setup>
import { Alert as AAlert, Button as AButton, Card as ACard, Form as AForm, FormItem as AFormItem, Input as AInput, RadioGroup as ARadioGroup, Space as ASpace, Avatar as AAvatar } from 'ant-design-vue'
import type { SiteSettings, SiteTheme } from '~/types/dashboard'

definePageMeta({
  layout: 'dashboard',
})

useHead({ title: '站点设置' })

const toast = useAdminNotice()
const colorMode = useColorMode()
const { adminFetch } = useAdminApi()
const { applySiteSettings } = useSiteSettings()

const themeOptions: Array<{ label: string, value: SiteTheme, icon: string }> = [
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

const changeTheme = (theme: SiteTheme) => {
  form.theme = theme
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

const confirmDiscardChanges = () => !isDirty.value || toast.confirm('站点设置尚未保存，确定要放弃修改吗？')
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
  <div>
    <DashboardPageHeader title="网站设置" description="配置公开页面的网站名称、标语、作者、头像和默认主题。"><AButton :loading="isLoading" :disabled="isSaving" @click="loadSettings">重新读取</AButton></DashboardPageHeader>
    <AAlert v-if="loadError" type="error" show-icon :message="loadError" class="mb-5" />
    <div class="grid gap-6 xl:grid-cols-[minmax(0,1fr)_320px]">
      <ACard title="基本设置">
        <AForm layout="vertical" :model="form" @finish="saveSettings">
          <AFormItem label="网站名称" name="title" required><AInput v-model:value="form.title" :maxlength="100" :disabled="isLoading || isSaving" /></AFormItem>
          <AFormItem label="网站标语" name="slogan" extra="留空可隐藏"><AInput v-model:value="form.slogan" :maxlength="200" :disabled="isLoading || isSaving" /></AFormItem>
          <AFormItem label="作者名称" name="author"><AInput v-model:value="form.author" :maxlength="100" :disabled="isLoading || isSaving" /></AFormItem>
          <AFormItem label="头像 URL" name="avatarUrl" extra="支持站内路径或完整 HTTP(S) 地址"><AInput v-model:value="form.avatarUrl" :maxlength="2048" :disabled="isLoading || isSaving" @change="avatarPreviewFailed = false" /></AFormItem>
          <AFormItem label="默认主题" name="theme"><ARadioGroup v-model:value="form.theme" :options="themeOptions" :disabled="isLoading || isSaving" /></AFormItem>
          <ASpace><AButton type="primary" html-type="submit" :loading="isSaving" :disabled="!isDirty || isLoading">保存设置</AButton><AButton :disabled="!isDirty || isLoading || isSaving" @click="resetForm">重置</AButton></ASpace>
          <p v-if="isDirty" class="admin-help mt-3">有未保存的修改</p>
        </AForm>
      </ACard>
      <ACard title="站点预览">
        <AAvatar :size="64" shape="square" :src="previewAvatarUrl" />
        <h2 class="mt-5 text-xl font-semibold break-words">{{ form.title || '网站名称' }}</h2>
        <p class="admin-help mt-3 break-words">{{ form.slogan || '这里显示网站标语' }}</p>
        <p class="admin-help mt-6">© {{ form.author || '作者' }}</p>
        <NuxtLink to="/" target="_blank"><AButton class="mt-4">查看公开页面</AButton></NuxtLink>
      </ACard>
    </div>
  </div>
</template>
