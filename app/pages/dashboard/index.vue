<script setup lang="ts">
import { Alert as AAlert, Button as AButton, Card as ACard, Statistic as AStatistic, Table as ATable, Space as ASpace } from 'ant-design-vue'
import type { Album, StorageBackend, StorageSettings } from '~/types/dashboard'

definePageMeta({ layout: 'dashboard' })
useHead({ title: '概览' })

const { adminFetch } = useAdminApi()
const albums = ref<Album[]>([])
const storage = ref<StorageSettings | null>(null)
const isLoading = ref(false)
const loadError = ref('')
const photoCount = computed(() => albums.value.reduce((total, album) => total + album.photoCount, 0))
const storageLabels: Record<StorageBackend, string> = { local: '本地存储', webdav: 'WebDAV', s3: 'S3 对象存储' }
const refreshAll = async () => {
  if (isLoading.value) return
  isLoading.value = true
  loadError.value = ''
  try {
    const [albumList, storageSettings] = await Promise.allSettled([
      adminFetch<Album[]>('/api/albums'),
      adminFetch<StorageSettings>('/api/settings/storage'),
    ])
    const failures: string[] = []
    if (albumList.status === 'fulfilled') albums.value = albumList.value
    else failures.push(`相册：${getAdminApiErrorMessage(albumList.reason)}`)
    if (storageSettings.status === 'fulfilled') storage.value = storageSettings.value
    else failures.push(`存储：${getAdminApiErrorMessage(storageSettings.reason)}`)
    loadError.value = failures.join('\n')
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
      <ACard title="从这里开始"><div class="admin-quick-actions"><NuxtLink to="/dashboard/albums"><Icon name="tabler:photo-plus" /><strong>添加与整理图片</strong><span>创建相册、上传、批量选图</span></NuxtLink><NuxtLink to="/dashboard/downloads"><Icon name="tabler:file-zip" /><strong>管理公开下载</strong><span>查看 ZIP 状态、批量设置</span></NuxtLink><NuxtLink to="/dashboard/tasks"><Icon name="tabler:activity" /><strong>查看后台任务</strong><span>生成进度、异常与待确认项</span></NuxtLink></div></ACard>
      <div class="grid gap-6 sm:grid-cols-3">
        <ACard><AStatistic title="相册总数" :value="albums.length" /></ACard>
        <ACard><AStatistic title="图片总数" :value="photoCount" /></ACard>
        <ACard><AStatistic title="当前图片存储" :value="storage ? storageLabels[storage.backend] : '—'" :value-style="{ fontSize: 22 }" /></ACard>
      </div>
      <ACard title="相册">
        <ATable :columns="columns" :data-source="albums" row-key="id" :loading="isLoading" :pagination="{ pageSize: 8, showSizeChanger: false }" :scroll="{ x: 600 }">
          <template #bodyCell="{ column, record }"><NuxtLink v-if="column.dataIndex === 'name'" :to="{ path: '/dashboard/albums', query: { album: record.id } }">{{ record.name }}</NuxtLink><template v-if="column.key === 'actions'"><ASpace><NuxtLink :to="{ path: '/dashboard/albums', query: { album: record.id } }">管理图片</NuxtLink><NuxtLink :to="{ path: '/dashboard/albums', query: { album: record.id, tab: 'downloads' } }">下载设置</NuxtLink></ASpace></template></template>
        </ATable>
      </ACard>
      <ACard title="常用操作"><ASpace wrap :size="16"><NuxtLink to="/dashboard/downloads"><AButton>管理本地 ZIP</AButton></NuxtLink><NuxtLink to="/dashboard/settings/storage"><AButton>存储与缓存维护</AButton></NuxtLink><NuxtLink to="/dashboard/settings/general"><AButton>修改网站信息</AButton></NuxtLink></ASpace><p class="admin-help mt-4">图片原件使用当前存储；三层浏览缓存及相册下载 ZIP 始终存放在服务器本地数据目录。</p></ACard>
    </div>
  </div>
</template>
