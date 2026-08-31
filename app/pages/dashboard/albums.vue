<script setup lang="ts">
import { Alert as AAlert, Button as AButton, Card as ACard, Checkbox as ACheckbox, DatePicker as ADatePicker, Empty as AEmpty, Form as AForm, FormItem as AFormItem, Image as AImage, Input as AInput, InputSearch as AInputSearch, Modal as AModal, Pagination as APagination, RadioGroup as ARadioGroup, Select as ASelect, Space as ASpace, Spin as ASpin, Table as ATable, Tabs as ATabs, TabPane as ATabPane, Tag as ATag, Textarea as ATextarea, Upload as AUpload, UploadDragger as AUploadDragger } from 'ant-design-vue'
import type { Album, AlbumCover, AlbumDeletionResult, AlbumDetail, Photo, PhotoDeletionResult } from '~/types/dashboard'
import { albumDraftOf, toggleVisibleSelection, validateAlbumDraft, type AlbumDraft } from '~~/shared/utils/admin-albums'

definePageMeta({ layout: 'dashboard' })
useHead({ title: '相册管理' })
const route = useRoute()
const router = useRouter()
const notice = useAdminNotice()
const { adminFetch } = useAdminApi()
const uploads = useAdminUploads()
const albums = ref<Album[]>([])
const photos = ref<Photo[]>([])
const selectedId = computed(() => typeof route.query.album === 'string' ? route.query.album : '')
const selectedAlbum = computed(() => albums.value.find(item => item.id === selectedId.value))
const tab = computed(() => ['details', 'downloads'].includes(String(route.query.tab)) ? String(route.query.tab) : 'photos')
const listState = useState('admin-album-list-view', () => ({ query: '', page: 1, selected: [] as string[] }))
const loading = ref(false)
const detailLoading = ref(false)
const ready = ref(false)
const error = ref('')
const detailError = ref('')
const mutation = ref('')
const coverBusy = ref(false)
const downloadBusy = ref(false)
const downloadDirty = ref(false)
const locked = computed(() => !!mutation.value || coverBusy.value || downloadBusy.value)
const draft = reactive<AlbumDraft>({ name: '', description: '', displayCreatedDate: null, photoDateStart: null, photoDateEnd: null })
const baseline = ref('')
const dirty = computed(() => !!baseline.value && JSON.stringify(draft) !== baseline.value)
const formError = ref('')
const createOpen = ref(false)
const newAlbum = reactive({ name: '', description: '' })
const createError = ref('')
const exportOpen = ref(false)
const orderMode = ref(false)
const orderIds = ref<string[]>([])
const orderDirty = computed(() => orderMode.value && orderIds.value.join() !== albums.value.map(item => item.id).join())
const photoQuery = ref('')
const photoFormat = ref('all')
const photoSort = ref('newest')
const photoView = useState<'grid' | 'table'>('admin-photo-view', () => 'grid')
const photoPage = ref(1)
const photoPageSize = 48
const selectedPhotos = ref<string[]>([])
const accept = '.png,.jpg,.jpeg,.jepg,.webp'
const filteredAlbums = computed(() => {
  const ordered = orderMode.value ? orderIds.value.map(id => albums.value.find(item => item.id === id)).filter((item): item is Album => !!item) : albums.value
  const query = listState.value.query.trim().toLocaleLowerCase()
  return orderMode.value ? ordered : ordered.filter(item => `${item.name} ${item.description}`.toLocaleLowerCase().includes(query))
})
const filteredPhotos = computed(() => {
  const query = photoQuery.value.trim().toLocaleLowerCase()
  const matches = photos.value.filter(item => item.originalName.toLocaleLowerCase().includes(query) && (photoFormat.value === 'all' || item.format === photoFormat.value))
  return matches.sort((a, b) => photoSort.value === 'name' ? a.originalName.localeCompare(b.originalName) : photoSort.value === 'size' ? b.byteSize - a.byteSize : b.createdAt - a.createdAt || b.id.localeCompare(a.id))
})
const pagePhotos = computed(() => filteredPhotos.value.slice((photoPage.value - 1) * photoPageSize, photoPage.value * photoPageSize))
const pageChecked = computed(() => pagePhotos.value.length > 0 && pagePhotos.value.every(photo => selectedPhotos.value.includes(photo.id)))
const pagePartial = computed(() => !pageChecked.value && pagePhotos.value.some(photo => selectedPhotos.value.includes(photo.id)))
const albumUploads = computed(() => uploads.state.value.items.filter(item => item.albumId === selectedId.value && ['queued', 'uploading'].includes(item.status)).length)
const albumColumns = computed(() => [
  ...(orderMode.value ? [{ title: '顺序', key: 'order', width: 180 }] : []),
  { title: '相册', key: 'album' }, { title: '图片', dataIndex: 'photoCount', width: 90 },
  ...(!orderMode.value ? [{ title: '操作', key: 'actions', width: 245 }] : []),
])
const photoColumns = [{ title: '图片', key: 'photo', width: 90 }, { title: '文件名', dataIndex: 'originalName' }, { title: '格式', dataIndex: 'format', width: 90 }, { title: '尺寸', key: 'dimensions', width: 140 }, { title: '大小', key: 'size', width: 110 }]
let detailSerial = 0
let disposed = false
let uploadRefresh: ReturnType<typeof setTimeout> | undefined
const applyDraft = (album: Album) => { Object.assign(draft, albumDraftOf(album)); baseline.value = JSON.stringify(draft); formError.value = '' }
const updateDate = (field: 'displayCreatedDate' | 'photoDateStart' | 'photoDateEnd', value: unknown) => { draft[field] = typeof value === 'string' && value ? value : null }
const applyAlbum = (album: Album) => {
  const index = albums.value.findIndex(item => item.id === album.id)
  if (index < 0) albums.value.push(album)
  else albums.value[index] = album
}
const applyCover = (id: string, cover: AlbumCover) => {
  const album = albums.value.find(item => item.id === id)
  if (album) Object.assign(album, cover)
}
const loadDetail = async (id = selectedId.value) => {
  const serial = ++detailSerial
  if (!id) { detailLoading.value = false; return }
  detailLoading.value = true
  try {
    const detail = await adminFetch<AlbumDetail>(`/api/albums/${encodeURIComponent(id)}`)
    if (disposed || serial !== detailSerial || selectedId.value !== id) return
    const { photos: loadedPhotos, ...album } = detail
    photos.value = loadedPhotos
    applyAlbum(album)
    if (!dirty.value) applyDraft(album)
    selectedPhotos.value = selectedPhotos.value.filter(photoId => loadedPhotos.some(photo => photo.id === photoId))
    detailError.value = ''
    ready.value = true
  } catch (cause) { if (serial === detailSerial) detailError.value = getAdminApiErrorMessage(cause) }
  finally { if (serial === detailSerial) detailLoading.value = false }
}
const loadAlbums = async () => {
  if (loading.value) return
  loading.value = true
  try {
    albums.value = await adminFetch<Album[]>('/api/albums')
    listState.value.selected = listState.value.selected.filter(id => albums.value.some(album => album.id === id))
    error.value = ''
  } catch (cause) { error.value = getAdminApiErrorMessage(cause) }
  finally { loading.value = false }
}
const navigateAlbum = (id = '', nextTab = 'photos') => router.push({ path: '/dashboard/albums', query: id ? { album: id, ...(nextTab === 'photos' ? {} : { tab: nextTab }) } : {} })
const leave = async () => {
  if (locked.value) { notice.add({ title: '正在提交操作，请稍候', color: 'warning' }); return false }
  return (!dirty.value && !downloadDirty.value && !orderDirty.value) || await notice.confirm('有未保存的修改，确定放弃修改并离开吗？')
}
onBeforeRouteLeave(leave)
onBeforeRouteUpdate((to, from) => to.query.album === from.query.album ? true : leave())
useEventListener('beforeunload', (event: BeforeUnloadEvent) => {
  if (dirty.value || downloadDirty.value || orderDirty.value || locked.value) { event.preventDefault(); event.returnValue = '' }
})
watch(selectedId, id => {
  clearTimeout(uploadRefresh)
  uploadRefresh = undefined
  baseline.value = ''; ready.value = false; photos.value = []; selectedPhotos.value = []; detailError.value = ''
  orderMode.value = false; orderIds.value = []
  downloadDirty.value = false; downloadBusy.value = false; coverBusy.value = false
  photoQuery.value = ''; photoFormat.value = 'all'; photoPage.value = 1
  void loadDetail(id)
})
watch([photoQuery, photoFormat, photoSort], () => { photoPage.value = 1 })
watch(() => filteredPhotos.value.length, count => { photoPage.value = Math.min(photoPage.value, Math.max(1, Math.ceil(count / photoPageSize))) })
watch(() => listState.value.query, () => { listState.value.page = 1 })
watch(() => filteredAlbums.value.length, count => { listState.value.page = Math.min(listState.value.page, Math.max(1, Math.ceil(count / 20))) })
watch(() => uploads.state.value.albumVersions[selectedId.value], () => {
  // A continuous upload must not postpone visible results until the queue ends.
  if (uploadRefresh) return
  uploadRefresh = setTimeout(() => { uploadRefresh = undefined; if (selectedId.value) void loadDetail() }, 1000)
})
onMounted(() => { void loadAlbums(); void loadDetail() })
onBeforeUnmount(() => { disposed = true; detailSerial++; clearTimeout(uploadRefresh) })

const create = async () => {
  if (locked.value) return
  const validation = validateAlbumDraft({ ...draft, ...newAlbum, displayCreatedDate: null, photoDateStart: null, photoDateEnd: null })
  if (validation) { createError.value = validation; return }
  mutation.value = 'create'
  let created: Album | undefined
  try {
    created = await adminFetch<Album>('/api/albums', { method: 'POST', body: { name: newAlbum.name.trim(), description: newAlbum.description.trim() } })
    applyAlbum(created); createOpen.value = false; newAlbum.name = ''; newAlbum.description = ''; createError.value = ''
    notice.add({ title: '相册已创建，可以上传图片了', color: 'success' })
  } catch (cause) { createError.value = getAdminApiErrorMessage(cause) }
  finally { mutation.value = '' }
  if (created) await navigateAlbum(created.id)
}
const save = async () => {
  if (locked.value || !ready.value || !dirty.value) return
  formError.value = validateAlbumDraft(draft) || ''
  if (formError.value) return
  mutation.value = 'save'
  try {
    const updated = await adminFetch<Album>(`/api/albums/${selectedId.value}`, { method: 'PATCH', body: { ...draft, name: draft.name.trim(), description: draft.description.trim() } })
    applyAlbum(updated); applyDraft(updated)
    notice.add({ title: '相册资料已保存', color: 'success' })
  } catch (cause) { formError.value = getAdminApiErrorMessage(cause) }
  finally { mutation.value = '' }
}
const queueFile = (file: File) => {
  if (!selectedAlbum.value || locked.value || !ready.value) return false
  if (!/\.(png|jpe?g|jepg|webp)$/i.test(file.name)) { notice.add({ title: `不支持此文件：${file.name}`, color: 'warning' }); return false }
  uploads.enqueue([file], { id: selectedAlbum.value.id, name: selectedAlbum.value.name })
  return false
}
const togglePhoto = (id: string) => { selectedPhotos.value = toggleVisibleSelection(selectedPhotos.value, [id], !selectedPhotos.value.includes(id)) }
const deletePhotos = async () => {
  if (locked.value || !selectedPhotos.value.length) return
  const ids = [...selectedPhotos.value]
  mutation.value = 'confirm-delete-photos'
  if (!await notice.confirm(`永久删除选中的 ${ids.length} 张图片？本地、S3 或 WebDAV 中的对应图片和缓存也会删除，无法撤销。`, true)) { mutation.value = ''; return }
  mutation.value = 'delete-photos'
  try {
    const result = await adminFetch<PhotoDeletionResult>('/api/photos/delete', { method: 'POST', body: { photoIds: ids } })
    await loadDetail()
    notice.add({ title: `已删除 ${result.deleted} 张图片`, description: result.cleanupPending ? `${result.cleanupPending} 个存储对象将在后台继续清理` : undefined, color: result.failures.length || result.cleanupPending ? 'warning' : 'success' })
  } catch (cause) { notice.add({ title: '删除失败，请刷新确认', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { mutation.value = '' }
}
const setCover = async () => {
  if (locked.value || selectedPhotos.value.length !== 1) return
  mutation.value = 'cover'
  try {
    const cover = await adminFetch<AlbumCover>(`/api/albums/${selectedId.value}/cover`, { method: 'PUT', body: { photoId: selectedPhotos.value[0] } })
    applyCover(selectedId.value, cover)
    notice.add({ title: '已设为相册封面', color: 'success' })
  } catch (cause) { notice.add({ title: '封面设置失败', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { mutation.value = '' }
}
const deleteAlbum = async () => {
  if (!selectedAlbum.value || locked.value) return
  if (albumUploads.value) { uploads.open.value = true; notice.add({ title: '请先完成或取消此相册的上传队列', color: 'warning' }); return }
  const album = selectedAlbum.value
  mutation.value = 'confirm-delete-album'
  if (!await notice.confirm(`永久删除相册「${album.name}」及其中全部 ${album.photoCount} 张图片？对应存储文件、封面和本地 ZIP 也会清理，无法撤销。`, true)) { mutation.value = ''; return }
  mutation.value = 'delete-album'
  let deleted = false
  try {
    const result = await adminFetch<AlbumDeletionResult>(`/api/albums/${album.id}`, { method: 'DELETE' })
    deleted = result.deleted
    if (deleted) {
      baseline.value = ''; downloadDirty.value = false
      albums.value = albums.value.filter(item => item.id !== album.id)
      listState.value.selected = listState.value.selected.filter(id => id !== album.id)
      notice.add({ title: `已删除「${album.name}」`, description: result.cleanupPending ? '剩余存储文件将在后台继续清理' : undefined, color: result.cleanupPending ? 'warning' : 'success' })
    }
  } catch (cause) { notice.add({ title: '删除失败，请刷新确认', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { mutation.value = '' }
  if (deleted) await navigateAlbum()
}
const startOrder = () => { orderIds.value = albums.value.map(item => item.id); orderMode.value = true }
const move = (id: string, delta: number) => {
  const index = orderIds.value.indexOf(id)
  const target = Math.max(0, Math.min(orderIds.value.length - 1, index + delta))
  if (index < 0 || index === target) return
  orderIds.value.splice(index, 1); orderIds.value.splice(target, 0, id)
}
const saveOrder = async () => {
  if (locked.value) return
  mutation.value = 'order'
  try { albums.value = await adminFetch<Album[]>('/api/albums/order', { method: 'POST', body: { albumIds: orderIds.value } }); orderMode.value = false; notice.add({ title: '相册顺序已保存', color: 'success' }) }
  catch (cause) { notice.add({ title: '顺序保存失败，请刷新后重试', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { mutation.value = '' }
}
const exportUrl = computed(() => `/api/albums/export?${new URLSearchParams({ albumIds: listState.value.selected.join(',') })}`)
</script>

<template>
  <div>
    <div v-if="selectedId" class="admin-workspace-back"><AButton type="link" class="admin-name-link" @click="navigateAlbum()">← 全部相册</AButton><span class="admin-help">/ {{ selectedAlbum?.name || '相册' }}</span></div>
    <DashboardPageHeader :title="selectedId ? (selectedAlbum?.name || '加载相册') : '相册管理'" :description="selectedId ? `${selectedAlbum?.photoCount || 0} 张图片 · 在同一个工作区完成图片、资料和下载管理` : '先创建相册，再添加图片。点击相册名称进入管理。'">
      <template v-if="!selectedId"><AButton :disabled="loading || locked || orderMode || albums.length < 2" @click="startOrder">调整顺序</AButton><AButton type="primary" :disabled="locked || orderMode" @click="createOpen = true">新建相册</AButton></template>
      <template v-else><AButton :href="`/albums/${selectedId}`" target="_blank">查看公开页面 ↗</AButton><AButton :loading="detailLoading" :disabled="locked" @click="loadDetail()">刷新</AButton><AUpload :accept="accept" multiple :show-upload-list="false" :before-upload="queueFile" :disabled="locked || !ready"><AButton type="primary" :disabled="locked || !ready"><Icon name="tabler:upload" /> 上传图片</AButton></AUpload></template>
    </DashboardPageHeader>
    <AAlert v-if="error" type="error" show-icon :message="error" class="mb-4" />
    <ACard v-if="!selectedId">
      <div class="admin-toolbar">
        <ASpace v-if="!orderMode" wrap><AInputSearch v-model:value="listState.query" placeholder="搜索名称或简介" aria-label="搜索相册" allow-clear style="width:260px;max-width:70vw" /><span class="admin-help">{{ filteredAlbums.length }} 个相册</span></ASpace>
        <ASpace v-else wrap><strong>调整首页展示顺序</strong><span class="admin-help">上移、下移或置顶，最后统一保存。</span></ASpace>
        <ASpace v-if="!orderMode"><AButton :loading="loading" @click="loadAlbums">刷新</AButton><AButton v-if="listState.selected.length" @click="exportOpen = true">导出原始文件（{{ listState.selected.length }}）</AButton><AButton v-if="listState.selected.length" @click="listState.selected = []">取消选择</AButton></ASpace>
        <ASpace v-else><AButton :disabled="locked" @click="orderMode = false">取消</AButton><AButton type="primary" :disabled="!orderDirty" :loading="mutation === 'order'" @click="saveOrder">保存顺序</AButton></ASpace>
      </div>
      <ATable :columns="albumColumns" :data-source="filteredAlbums" row-key="id" :loading="loading" :row-selection="orderMode ? undefined : { selectedRowKeys: listState.selected, onChange: (keys: (string | number)[]) => listState.selected = keys.map(String), preserveSelectedRowKeys: true }" :pagination="orderMode ? false : { current: listState.page, onChange: (page: number) => listState.page = page, pageSize: 20, showSizeChanger: false }" :scroll="{ x: 700 }">
        <template #bodyCell="{ column, record, index }">
          <ASpace v-if="column.key === 'order'" :size="2"><span class="admin-help mr-2">{{ index + 1 }}</span><AButton size="small" :disabled="locked || index === 0" :aria-label="`上移${record.name}`" @click="move(record.id, -1)">↑</AButton><AButton size="small" :disabled="locked || index === albums.length - 1" :aria-label="`下移${record.name}`" @click="move(record.id, 1)">↓</AButton><AButton type="link" size="small" :disabled="locked || index === 0" @click="move(record.id, -albums.length)">置顶</AButton></ASpace>
          <div v-if="column.key === 'album'" class="admin-album-cell"><img v-if="record.coverUrl" :src="record.coverUrl" alt="" loading="lazy" /><div v-else class="admin-album-placeholder"><Icon name="tabler:album" /></div><div><AButton type="link" class="admin-name-link" :disabled="orderMode" @click="navigateAlbum(record.id)">{{ record.name }}</AButton><div class="admin-help admin-album-description">{{ record.description || '暂无简介' }}</div></div></div>
          <ASpace v-if="column.key === 'actions'" :size="4"><AButton type="link" @click="navigateAlbum(record.id)">管理图片</AButton><AButton type="link" @click="navigateAlbum(record.id, 'details')">资料</AButton><AButton type="link" @click="navigateAlbum(record.id, 'downloads')">下载</AButton></ASpace>
        </template>
      </ATable>
    </ACard>
    <template v-else>
      <AAlert v-if="detailError" type="error" show-icon :message="detailError" class="mb-4" />
      <ASpin v-if="!ready && detailLoading" class="my-8" tip="加载相册…" />
      <AAlert v-if="!selectedAlbum && !loading && !detailLoading && !detailError" type="warning" message="相册不存在，请返回列表重新选择。" />
      <ATabs v-if="selectedAlbum && ready" :active-key="tab" @change="(key: string | number) => navigateAlbum(selectedId, String(key))">
        <ATabPane key="photos" :tab="`图片管理（${photos.length}）`">
          <AAlert v-if="albumUploads" type="info" show-icon class="mb-4" :message="`${albumUploads} 张图片等待上传或处理中；可切换页面，成功入库后自动显示。`"><template #action><AButton size="small" @click="uploads.open.value = true">查看队列</AButton></template></AAlert>
          <ACard>
            <template v-if="!photos.length && !detailError"><AUploadDragger :accept="accept" multiple :show-upload-list="false" :before-upload="queueFile" :disabled="locked"><p class="ant-upload-drag-icon"><Icon name="tabler:cloud-upload" /></p><p class="ant-upload-text">拖入图片，或点击开始上传</p><p class="ant-upload-hint">选择后自动上传，7 并发；入库时自动生成三层预览。</p></AUploadDragger></template>
            <template v-else>
              <AUploadDragger class="admin-compact-upload" :accept="accept" multiple :show-upload-list="false" :before-upload="queueFile" :disabled="locked"><span><Icon name="tabler:cloud-upload" /> 拖入更多图片，或点击选择 · 自动加入上传队列</span></AUploadDragger>
              <div class="admin-toolbar"><ASpace wrap><AInputSearch v-model:value="photoQuery" placeholder="搜索文件名" aria-label="搜索图片" allow-clear style="width:230px" /><ASelect v-model:value="photoFormat" aria-label="筛选图片格式" :options="[{ label: '全部格式', value: 'all' }, { label: 'PNG', value: 'png' }, { label: 'JPG / JPEG', value: 'jpg' }, { label: 'WebP', value: 'webp' }]" style="width:130px" /><ASelect v-model:value="photoSort" aria-label="图片排序" :options="[{ label: '最近上传', value: 'newest' }, { label: '文件名称', value: 'name' }, { label: '文件大小', value: 'size' }]" style="width:120px" /></ASpace><ARadioGroup v-model:value="photoView" option-type="button" aria-label="图片视图" :options="[{ label: '网格', value: 'grid' }, { label: '列表', value: 'table' }]" /></div>
              <div class="admin-selection-bar"><ASpace wrap><ACheckbox :checked="pageChecked" :indeterminate="pagePartial" :disabled="locked || !pagePhotos.length" @change="(event: { target: { checked: boolean } }) => selectedPhotos = toggleVisibleSelection(selectedPhotos, pagePhotos.map(photo => photo.id), event.target.checked)">本页全选</ACheckbox><span>已选 {{ selectedPhotos.length }} / {{ photos.length }}</span><AButton type="link" size="small" :disabled="locked || !filteredPhotos.length" @click="selectedPhotos = toggleVisibleSelection(selectedPhotos, filteredPhotos.map(photo => photo.id), true)">选择全部筛选结果（{{ filteredPhotos.length }}）</AButton><AButton v-if="selectedPhotos.length" type="link" size="small" :disabled="locked" @click="selectedPhotos = []">取消选择</AButton></ASpace><ASpace v-if="selectedPhotos.length"><AButton :disabled="locked || selectedPhotos.length !== 1" @click="setCover">设为封面</AButton><AButton danger :loading="mutation === 'delete-photos'" :disabled="locked && mutation !== 'delete-photos'" @click="deletePhotos">删除（{{ selectedPhotos.length }}）</AButton></ASpace></div>
              <div v-if="photoView === 'grid' && pagePhotos.length" class="admin-photo-grid">
                <article v-for="photo in pagePhotos" :key="photo.id" class="admin-photo-tile" :class="{ selected: selectedPhotos.includes(photo.id) }">
                  <AImage :src="`/api/photos/${photo.id}/thumbnail?v=grid2`" :alt="photo.originalName" :preview="{ src: `/api/photos/${photo.id}/preview` }" loading="lazy" />
                  <ACheckbox class="admin-photo-check" :checked="selectedPhotos.includes(photo.id)" :aria-label="`选择图片：${photo.originalName}`" :disabled="locked" @change="togglePhoto(photo.id)" />
                  <ATag v-if="selectedAlbum.coverPhotoId === photo.id" class="admin-cover-label" color="blue">封面</ATag>
                  <button type="button" class="admin-photo-caption" :title="photo.originalName" :disabled="locked" @click="togglePhoto(photo.id)"><span>{{ photo.originalName }}</span><small>{{ photo.format.toUpperCase() }} · {{ adminBytes(photo.byteSize) }}</small></button>
                </article>
              </div>
              <ATable v-else-if="photoView === 'table'" :columns="photoColumns" :data-source="pagePhotos" row-key="id" size="middle" :pagination="false" :row-selection="{ selectedRowKeys: selectedPhotos, onChange: (keys: (string | number)[]) => selectedPhotos = keys.map(String), preserveSelectedRowKeys: true, getCheckboxProps: () => ({ disabled: locked }) }" :scroll="{ x: 700 }">
                <template #bodyCell="{ column, record }"><AImage v-if="column.key === 'photo'" :width="56" :height="56" style="object-fit:cover" :src="`/api/photos/${record.id}/thumbnail?v=grid2`" :preview="{ src: `/api/photos/${record.id}/preview` }" :alt="record.originalName" /><span v-if="column.key === 'size'">{{ adminBytes(record.byteSize) }}</span><span v-if="column.key === 'dimensions'">{{ record.width }} × {{ record.height }}</span></template>
              </ATable>
              <AEmpty v-else description="没有符合条件的图片" />
              <div class="admin-pagination"><span class="admin-help">{{ filteredPhotos.length }} 张符合条件 · 每页 {{ photoPageSize }} 张</span><APagination v-model:current="photoPage" :page-size="photoPageSize" :total="filteredPhotos.length" :show-size-changer="false" show-quick-jumper /></div>
            </template>
          </ACard>
        </ATabPane>
        <ATabPane key="details" tab="相册资料">
          <div class="admin-stack">
            <ACard title="名称、简介与展示日期">
              <AForm layout="vertical" :model="draft" :disabled="locked" @finish="save">
                <div class="admin-form-grid"><AFormItem label="相册名称" required><AInput v-model:value="draft.name" :maxlength="100" show-count /></AFormItem><AFormItem label="展示创建日期" extra="留空使用真实创建日期。"><ADatePicker :value="draft.displayCreatedDate || undefined" value-format="YYYY-MM-DD" placeholder="自动日期" @update:value="(value: unknown) => updateDate('displayCreatedDate', value)" /></AFormItem></div>
                <AFormItem label="相册简介"><ATextarea v-model:value="draft.description" :rows="3" :maxlength="1000" show-count placeholder="显示在公开相册页面的介绍" /></AFormItem>
                <div class="admin-form-grid"><AFormItem label="图片开始日期"><ADatePicker :value="draft.photoDateStart || undefined" value-format="YYYY-MM-DD" placeholder="自动日期" @update:value="(value: unknown) => updateDate('photoDateStart', value)" /></AFormItem><AFormItem label="图片结束日期"><ADatePicker :value="draft.photoDateEnd || undefined" value-format="YYYY-MM-DD" placeholder="自动日期" @update:value="(value: unknown) => updateDate('photoDateEnd', value)" /></AFormItem></div>
                <AButton type="link" class="admin-name-link" @click="draft.displayCreatedDate = null; draft.photoDateStart = null; draft.photoDateEnd = null">恢复自动日期（保存后生效）</AButton>
                <AAlert v-if="formError" type="error" show-icon :message="formError" class="mt-4" />
                <div class="admin-save-bar"><span :class="dirty ? 'admin-unsaved' : 'admin-help'">{{ dirty ? '有未保存的修改' : '所有资料已保存' }}</span><ASpace><AButton :disabled="!dirty || locked" @click="applyDraft(selectedAlbum)">放弃修改</AButton><AButton html-type="submit" type="primary" :disabled="!dirty" :loading="mutation === 'save'">保存相册资料</AButton></ASpace></div>
              </AForm>
            </ACard>
            <DashboardAlbumCoverEditor :key="selectedId" :album="selectedAlbum" :photos="photos" :disabled="locked" @saved="applyCover" @busy="coverBusy = $event" />
            <ACard size="small"><div class="admin-toolbar" style="margin:0"><div><strong>删除相册</strong><p class="admin-help" style="margin:4px 0 0">将删除相册及其中全部图片、封面和压缩包，无法撤销。</p></div><AButton danger :disabled="locked || dirty || downloadDirty" :loading="mutation === 'delete-album'" @click="deleteAlbum">删除此相册</AButton></div></ACard>
          </div>
        </ATabPane>
        <ATabPane key="downloads" tab="公开下载"><DashboardDownloadManager :key="selectedId" :album-id="selectedId" embedded @dirty="downloadDirty = $event" @busy="downloadBusy = $event" /></ATabPane>
      </ATabs>
    </template>
    <AModal v-model:open="createOpen" title="新建相册" :footer="null" :closable="!locked" :mask-closable="!locked" :keyboard="!locked">
      <AForm layout="vertical" :model="newAlbum" :disabled="locked" @finish="create"><AFormItem label="相册名称" required><AInput v-model:value="newAlbum.name" :maxlength="100" placeholder="例如：2026 夏日旅行" /></AFormItem><AFormItem label="简介（选填）"><ATextarea v-model:value="newAlbum.description" :rows="3" :maxlength="1000" /></AFormItem><AAlert v-if="createError" type="error" :message="createError" class="mb-4" /><div class="admin-dialog-actions"><AButton :disabled="locked" @click="createOpen = false">取消</AButton><AButton type="primary" html-type="submit" :loading="mutation === 'create'">创建并上传图片</AButton></div></AForm>
    </AModal>
    <AModal v-model:open="exportOpen" title="管理员导出原始文件" :footer="null"><p>导出选中的 {{ listState.selected.length }} 个相册，保持入库文件格式，不改变公开下载设置。</p><p class="admin-help">一个相册生成一个 ZIP；多个相册生成一个外层 ZIP，其中每个相册各一个 ZIP。大相册需要等待服务器打包。</p><div class="admin-dialog-actions"><AButton @click="exportOpen = false">取消</AButton><AButton type="primary" :href="exportUrl" :disabled="!listState.selected.length" @click="exportOpen = false">下载原始文件</AButton></div></AModal>
  </div>
</template>
