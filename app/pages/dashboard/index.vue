<script setup lang="ts">
import type { Album, Photo, StorageBackend, StorageSettings } from '~/types/dashboard'

definePageMeta({ layout: 'dashboard' })
useHead({ title: '概览' })

const { adminFetch } = useAdminApi()
const albums = ref<Album[]>([])
const photos = ref<Photo[]>([])
const storage = ref<StorageSettings | null>(null)
const isLoading = ref(false)
const loadError = ref('')
const totalBytes = computed(() => photos.value.reduce((total, photo) => total + photo.byteSize, 0))
const storageLabels: Record<StorageBackend, string> = { local: '本地存储', webdav: 'WebDAV', s3: 'S3 对象存储' }
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
    const [albumList, photoList, storageSettings] = await Promise.all([
      adminFetch<Album[]>('/api/albums'),
      adminFetch<Photo[]>('/api/photos'),
      adminFetch<StorageSettings>('/api/settings/storage'),
    ])
    albums.value = albumList
    photos.value = photoList
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
        <template #right><UButton icon="tabler:refresh" color="neutral" variant="ghost" :loading="isLoading" @click="refreshAll">刷新</UButton></template>
      </UDashboardNavbar>
    </template>
    <template #body>
      <div class="dashboard-panel-body flex flex-col gap-6">
        <UAlert v-if="loadError" color="error" variant="subtle" icon="tabler:alert-circle" title="概览数据加载失败" :description="loadError" />
        <DashboardPageHero eyebrow="工作台" title="相簿与图片概览" description="上传后自动生成三层浏览图；前台浏览不再直接读取体积庞大的原始文件。" icon="tabler:layout-dashboard">
          <template #actions><UButton to="/dashboard/albums" icon="tabler:plus">新建相簿</UButton><UButton to="/dashboard/settings/storage" color="neutral" variant="soft" icon="tabler:photo-cog">派生图维护</UButton></template>
        </DashboardPageHero>
        <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <DashboardMetricCard label="相簿空间" :value="albums.length" icon="tabler:album" tone="info" hint="进入相簿工作台" to="/dashboard/albums" />
          <DashboardMetricCard label="图片总数" :value="photos.length" icon="tabler:photo" tone="success" :hint="formatBytes(totalBytes)" to="/dashboard/albums" />
          <DashboardMetricCard label="浏览层级" value="3 层" icon="tabler:layers-subtract" tone="warning" hint="PNG / 1.5 MB / 5 MB" to="/dashboard/settings/storage" />
          <DashboardMetricCard label="当前存储" :value="storage ? storageLabels[storage.backend] : '—'" icon="tabler:database" tone="neutral" hint="配置保存在后台" to="/dashboard/settings/storage" />
        </div>
        <div class="grid grid-cols-1 gap-4 xl:grid-cols-5">
          <section class="dashboard-section overflow-hidden xl:col-span-3">
            <div class="border-b border-default px-5 py-4"><h2 class="font-semibold text-highlighted">图片交付链路</h2><p class="mt-1 text-sm text-muted">每张图片上传后并发生成，失败时访问接口也会自动补齐</p></div>
            <div class="grid gap-3 p-5 sm:grid-cols-3">
              <div class="rounded-xl bg-elevated p-4"><Icon name="tabler:photo" class="size-6 text-primary" /><p class="mt-3 font-medium">网格缩略图</p><p class="mt-1 text-xs leading-5 text-muted">320px PNG，优先铺满相簿页面。</p></div>
              <div class="rounded-xl bg-elevated p-4"><Icon name="tabler:photo-search" class="size-6 text-info" /><p class="mt-3 font-medium">默认查看图</p><p class="mt-1 text-xs leading-5 text-muted">WebP 严格不超过 1.5 MB。</p></div>
              <div class="rounded-xl bg-elevated p-4"><Icon name="tabler:zoom-in-area" class="size-6 text-success" /><p class="mt-3 font-medium">手动高清图</p><p class="mt-1 text-xs leading-5 text-muted">点击后加载，WebP 不超过 5 MB。</p></div>
            </div>
          </section>
          <section class="dashboard-section overflow-hidden xl:col-span-2">
            <div class="border-b border-default px-5 py-4"><h2 class="font-semibold text-highlighted">常用操作</h2><p class="mt-1 text-sm text-muted">日常管理只保留高频入口</p></div>
            <div class="space-y-2 p-3">
              <NuxtLink to="/dashboard/albums" class="group flex items-center gap-3 rounded-xl p-3 transition hover:bg-elevated"><span class="flex size-10 items-center justify-center rounded-xl bg-primary/10 text-primary"><Icon name="tabler:folder-plus" class="size-5" /></span><span class="min-w-0 flex-1"><span class="block font-medium text-highlighted">创建相簿并上传</span><span class="mt-0.5 block text-xs text-muted">上传时自动处理三层浏览图</span></span><Icon name="tabler:chevron-right" class="size-4 text-dimmed" /></NuxtLink>
              <NuxtLink to="/dashboard/settings/storage" class="group flex items-center gap-3 rounded-xl p-3 transition hover:bg-elevated"><span class="flex size-10 items-center justify-center rounded-xl bg-warning/10 text-warning"><Icon name="tabler:refresh" class="size-5" /></span><span class="min-w-0 flex-1"><span class="block font-medium text-highlighted">重建全站派生图</span><span class="mt-0.5 block text-xs text-muted">为旧版本图片补齐三层缓存</span></span><Icon name="tabler:chevron-right" class="size-4 text-dimmed" /></NuxtLink>
              <NuxtLink to="/dashboard/settings/general" class="group flex items-center gap-3 rounded-xl p-3 transition hover:bg-elevated"><span class="flex size-10 items-center justify-center rounded-xl bg-info/10 text-info"><Icon name="tabler:palette" class="size-5" /></span><span class="min-w-0 flex-1"><span class="block font-medium text-highlighted">网站外观</span><span class="mt-0.5 block text-xs text-muted">修改名称、标语、头像和主题</span></span><Icon name="tabler:chevron-right" class="size-4 text-dimmed" /></NuxtLink>
            </div>
          </section>
        </div>
      </div>
    </template>
  </UDashboardPanel>
</template>
