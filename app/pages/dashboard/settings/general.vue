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
  <UDashboardPanel :ui="{ body: 'p-0 sm:p-0' }">
    <template #header>
      <UDashboardNavbar title="网站设置">
        <template #right>
          <UButton icon="tabler:refresh" color="neutral" variant="ghost" :loading="isLoading" :disabled="isSaving" @click="loadSettings">重新读取</UButton>
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="dashboard-panel-body space-y-6">
        <DashboardPageHero eyebrow="网站设置" title="公开站点信息" description="集中设置网站名称、标语、作者、头像和默认主题；保存后立即用于公开页面。" icon="tabler:world-cog">
          <template #actions><UButton to="/" target="_blank" color="neutral" variant="soft" icon="tabler:external-link">查看公开页面</UButton></template>
        </DashboardPageHero>

        <UAlert v-if="loadError" color="error" variant="subtle" icon="tabler:alert-circle" title="站点设置加载失败" :description="loadError" />

        <div class="grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_340px]">
          <form id="site-settings-form" class="dashboard-section overflow-hidden" @submit.prevent="saveSettings">
            <header class="flex items-start gap-3 border-b border-default px-5 py-4 sm:px-6">
              <span class="flex size-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary"><Icon name="tabler:world-cog" class="size-5" /></span>
              <div><h2 class="font-semibold text-highlighted">站点身份</h2><p class="mt-1 text-sm text-muted">访客在首页、相簿页和浏览器标题中看到的内容</p></div>
            </header>

            <div class="space-y-5 p-5 sm:p-6">
              <UFormField label="网站名称" description="显示在公开页面、浏览器标题和管理后台。" required><UInput v-model="form.title" maxlength="100" icon="tabler:world" size="lg" class="w-full" :disabled="isLoading || isSaving" /></UFormField>
              <UFormField label="网站标语" description="显示在相簿首页和照片页；留空可隐藏。"><UInput v-model="form.slogan" maxlength="200" icon="tabler:quote" size="lg" class="w-full" :disabled="isLoading || isSaving" /></UFormField>
              <div class="grid gap-5 md:grid-cols-2">
                <UFormField label="作者名称" description="用于照片页底部版权信息。"><UInput v-model="form.author" maxlength="100" icon="tabler:user" class="w-full" :disabled="isLoading || isSaving" /></UFormField>
                <UFormField label="头像 URL" description="站内路径或完整 HTTP(S) URL。"><UInput v-model="form.avatarUrl" maxlength="2048" icon="tabler:photo" class="w-full" :disabled="isLoading || isSaving" @update:model-value="avatarPreviewFailed = false" /></UFormField>
              </div>
            </div>

            <section class="border-t border-default p-5 sm:p-6">
              <div class="mb-4"><h3 class="font-semibold text-highlighted">默认主题</h3><p class="mt-1 text-sm text-muted">访客首次打开网站时采用，仍可在照片页临时切换。</p></div>
              <div class="grid gap-3 sm:grid-cols-3">
                <button v-for="option in themeOptions" :key="option.value" type="button" class="flex items-center gap-3 rounded-xl border p-3 text-left transition" :class="form.theme === option.value ? 'border-primary/30 bg-primary/10 text-primary' : 'border-default hover:bg-elevated'" :disabled="isLoading || isSaving" @click="changeTheme(option.value)">
                  <span class="flex size-9 items-center justify-center rounded-lg bg-elevated"><Icon :name="option.icon" class="size-4" /></span><span class="text-sm font-medium">{{ option.label }}</span><Icon v-if="form.theme === option.value" name="tabler:check" class="ml-auto size-4" />
                </button>
              </div>
            </section>
          </form>

          <aside class="dashboard-section overflow-hidden xl:sticky xl:top-4">
            <div class="border-b border-default px-5 py-4"><h2 class="font-semibold text-highlighted">即时预览</h2><p class="mt-1 text-sm text-muted">保存前先确认公开站点身份</p></div>
            <div class="p-5">
              <div class="rounded-xl border border-default bg-muted p-5">
                <div>
                  <img v-if="!avatarPreviewFailed" :src="previewAvatarUrl" alt="头像预览" class="size-14 rounded-xl bg-default object-cover ring-1 ring-default" @error="avatarPreviewFailed = true">
                  <span v-else class="flex size-14 items-center justify-center rounded-xl bg-elevated text-muted"><Icon name="tabler:photo-off" class="size-6" /></span>
                  <h3 class="mt-5 break-words text-2xl font-semibold tracking-tight text-highlighted">{{ form.title.trim() || '网站名称' }}</h3>
                  <p class="mt-2 break-words text-sm leading-6 text-muted">{{ form.slogan.trim() || '这里会显示网站标语' }}</p>
                  <div class="mt-6 flex items-center gap-2 border-t border-default/60 pt-4 text-xs text-muted"><Icon name="tabler:copyright" class="size-4" /><span>{{ form.author.trim() || form.title.trim() || '作者' }}</span></div>
                </div>
              </div>
              <div class="mt-4 flex items-center justify-between rounded-xl bg-elevated px-3 py-2.5 text-xs"><span class="text-muted">默认主题</span><span class="font-medium text-highlighted">{{ themeOptions.find(option => option.value === form.theme)?.label }}</span></div>
              <p v-if="avatarPreviewFailed" class="mt-3 flex items-start gap-2 text-xs leading-5 text-warning"><Icon name="tabler:alert-circle" class="mt-0.5 size-4 shrink-0" />头像地址无法加载，保存前请检查 URL。</p>
            </div>
          </aside>
        </div>

        <div class="dashboard-toolbar sticky bottom-3 z-10 shadow-md">
          <p class="flex items-center gap-2 text-sm" :class="isDirty ? 'text-warning' : 'text-muted'"><Icon :name="isDirty ? 'tabler:edit-circle' : 'tabler:circle-check'" class="size-5" />{{ isDirty ? '有未保存的更改，公开页面尚未更新' : '当前设置已保存' }}</p>
          <div class="flex justify-end gap-2"><UButton color="neutral" variant="ghost" :disabled="!isDirty || isLoading || isSaving" @click="resetForm">放弃修改</UButton><UButton form="site-settings-form" type="submit" icon="tabler:device-floppy" :loading="isSaving" :disabled="!isDirty || isLoading">保存并发布</UButton></div>
        </div>
      </div>
    </template>
  </UDashboardPanel>
</template>
