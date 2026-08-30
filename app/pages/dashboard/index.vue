<script setup lang="ts">
import { Alert as AAlert, Button as AButton, Card as ACard, Statistic as AStatistic, Table as ATable, Tag as ATag, Space as ASpace } from 'ant-design-vue'
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
const columns = [{ title: '相册名称', dataIndex: 'name' }, { title: '图片数量', dataIndex: 'photoCount', width: 120 }, { title: '简介', dataIndex: 'description', ellipsis: true }, { title: '操作', key: 'actions', width: 160 }]
</script>

<template>
  <div>
    <DashboardPageHeader title="概览" description="查看相册、图片和存储状态，快速进入日常管理。"><AButton :loading="isLoading" @click="refreshAll">刷新</AButton><NuxtLink to="/dashboard/albums"><AButton type="primary">管理相册</AButton></NuxtLink></DashboardPageHeader>
    <div class="admin-stack">
      <AAlert v-if="loadError" type="error" show-icon :message="loadError" />
      <div class="grid gap-6 sm:grid-cols-2 xl:grid-cols-4">
        <ACard><AStatistic title="相册总数" :value="albums.length" /></ACard>
        <ACard><AStatistic title="图片总数" :value="photos.length" /></ACard>
        <ACard><AStatistic title="原图总大小" :value="formatBytes(totalBytes)" /></ACard>
        <ACard><AStatistic title="当前图片存储" :value="storage ? storageLabels[storage.backend] : '—'" :value-style="{ fontSize: 22 }" /></ACard>
      </div>
      <ACard title="相册">
        <ATable :columns="columns" :data-source="albums" row-key="id" :loading="isLoading" :pagination="{ pageSize: 8, showSizeChanger: false }" :scroll="{ x: 600 }">
          <template #bodyCell="{ column, record }"><template v-if="column.key === 'actions'"><ASpace><NuxtLink :to="'/albums/' + record.id" target="_blank">查看</NuxtLink><NuxtLink :to="'/dashboard/downloads?album=' + record.id">下载设置</NuxtLink></ASpace></template></template>
        </ATable>
      </ACard>
      <ACard title="常用操作"><ASpace wrap :size="16"><NuxtLink to="/dashboard/downloads"><AButton>管理本地 ZIP</AButton></NuxtLink><NuxtLink to="/dashboard/settings/storage"><AButton>存储与缓存维护</AButton></NuxtLink><NuxtLink to="/dashboard/settings/general"><AButton>修改网站信息</AButton></NuxtLink></ASpace><p class="admin-help mt-4">图片原件使用当前存储；三层浏览缓存及相册下载 ZIP 始终存放在服务器本地数据目录。</p></ACard>
    </div>
  </div>
</template>
