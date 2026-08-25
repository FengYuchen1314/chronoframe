<script lang="ts" setup>
import type { Album, AlbumDetail, Photo } from '~/types/dashboard'

definePageMeta({
  layout: 'dashboard',
})

useHead({ title: '相簿' })

const toast = useToast()
const { adminFetch } = useAdminApi()

const albums = ref<Album[]>([])
const selectedAlbumId = ref('')
const photos = ref<Photo[]>([])
const activeWorkspaceTab = ref<'photos' | 'details' | 'export'>('photos')
const albumQuery = ref('')
const isCreateDialogOpen = ref(false)
const isOrderMode = ref(false)
const newAlbumName = ref('')
const newAlbumDescription = ref('')
const selectedFiles = ref<File[]>([])
const exportAlbumIds = ref<string[]>([])
const isStartingExport = ref(false)
const uploadInput = ref<HTMLInputElement | null>(null)
const albumDateDraft = reactive({
  displayCreatedDate: '',
  photoDateStart: '',
  photoDateEnd: '',
})
const savedAlbumDateDraft = reactive({
  displayCreatedDate: '',
  photoDateStart: '',
  photoDateEnd: '',
})
const dateDraftAlbumId = ref('')
const albumDescriptionDraft = ref('')
const savedAlbumDescriptionDraft = ref('')
const descriptionDraftAlbumId = ref('')

const isLoadingAlbums = ref(false)
const isLoadingPhotos = ref(false)
const isCreating = ref(false)
const isUploading = ref(false)
const isSavingAlbumDates = ref(false)
const isSavingAlbumDescription = ref(false)
const isReorderingAlbums = ref(false)
const isAlbumDetailReady = ref(false)
const albumError = ref('')
const photoError = ref('')
let detailRequestSerial = 0

const selectedAlbum = computed(() =>
  albums.value.find(album => album.id === selectedAlbumId.value) || null,
)
const filteredAlbums = computed(() => {
  const query = albumQuery.value.trim().toLocaleLowerCase()
  if (!query) return albums.value
  return albums.value.filter(album =>
    album.name.toLocaleLowerCase().includes(query)
    || album.description.toLocaleLowerCase().includes(query),
  )
})
const albumDatesDirty = computed(() =>
  dateDraftAlbumId.value === selectedAlbumId.value
  && (
    albumDateDraft.displayCreatedDate !== savedAlbumDateDraft.displayCreatedDate
    || albumDateDraft.photoDateStart !== savedAlbumDateDraft.photoDateStart
    || albumDateDraft.photoDateEnd !== savedAlbumDateDraft.photoDateEnd
  ),
)
const albumDescriptionDirty = computed(() =>
  descriptionDraftAlbumId.value === selectedAlbumId.value
  && albumDescriptionDraft.value !== savedAlbumDescriptionDraft.value,
)
const albumMetadataDirty = computed(() => albumDatesDirty.value || albumDescriptionDirty.value)
const albumHasCustomDates = computed(() => Boolean(
  selectedAlbum.value?.displayCreatedDate
  || selectedAlbum.value?.photoDateStart
  || selectedAlbum.value?.photoDateEnd,
))
const hasActiveMutation = computed(() =>
  isCreating.value
  || isUploading.value
  || isSavingAlbumDates.value
  || isSavingAlbumDescription.value
  || isReorderingAlbums.value,
)
const isAlbumInteractionLocked = computed(() =>
  isLoadingAlbums.value || isLoadingPhotos.value || hasActiveMutation.value,
)

const selectedBytes = computed(() =>
  selectedFiles.value.reduce((total, file) => total + file.size, 0),
)
const selectedExportAlbums = computed(() =>
  albums.value.filter(album => exportAlbumIds.value.includes(album.id)),
)

const workspaceTabs = [
  { value: 'photos' as const, label: '图片与上传', icon: 'tabler:photo' },
  { value: 'details' as const, label: '资料与日期', icon: 'tabler:edit' },
  { value: 'export' as const, label: '打包下载', icon: 'tabler:file-zip' },
]

const openCreateDialog = () => {
  if (albumMetadataDirty.value) {
    toast.add({ title: '请先保存或放弃当前相簿的资料修改', color: 'warning' })
    return
  }
  isCreateDialogOpen.value = true
}

const closeCreateDialog = () => {
  if (isCreating.value) return
  isCreateDialogOpen.value = false
  newAlbumName.value = ''
  newAlbumDescription.value = ''
}

const openExportWorkspace = (includeCurrent = false) => {
  activeWorkspaceTab.value = 'export'
  if (includeCurrent && selectedAlbumId.value && !exportAlbumIds.value.includes(selectedAlbumId.value)) {
    exportAlbumIds.value = [...exportAlbumIds.value, selectedAlbumId.value]
  }
}

const toggleExportAlbum = (albumId: string, checked: boolean) => {
  exportAlbumIds.value = checked
    ? [...new Set([...exportAlbumIds.value, albumId])]
    : exportAlbumIds.value.filter(id => id !== albumId)
}

const handleExportAlbumToggle = (albumId: string, event: Event) => {
  toggleExportAlbum(albumId, (event.target as HTMLInputElement | null)?.checked === true)
}

const startAlbumExport = () => {
  if (!exportAlbumIds.value.length || isStartingExport.value) return
  isStartingExport.value = true
  const query = new URLSearchParams({ albumIds: exportAlbumIds.value.join(',') })
  const link = document.createElement('a')
  link.href = `/api/albums/export?${query.toString()}`
  link.rel = 'noopener'
  document.body.appendChild(link)
  link.click()
  link.remove()
  toast.add({
    title: '已开始准备相簿压缩包',
    description: exportAlbumIds.value.length === 1
      ? `将下载「${selectedExportAlbums.value[0]?.name || '相簿'}」的 ZIP。`
      : `将下载一个外层 ZIP，其中包含 ${exportAlbumIds.value.length} 个相簿 ZIP。`,
    color: 'success',
  })
  window.setTimeout(() => {
    isStartingExport.value = false
  }, 3000)
}

const formatBytes = (bytes: number) => {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 1)} ${units[index]}`
}

const timestampToDateInput = (timestamp: number) => {
  const date = new Date(timestamp * 1000)
  if (Number.isNaN(date.getTime())) return ''
  const year = String(date.getFullYear()).padStart(4, '0')
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

const applyAlbumDateDraft = (detail: AlbumDetail, force = false) => {
  if (!force && dateDraftAlbumId.value === detail.id && albumDatesDirty.value) return

  const fallbackCreatedDate = timestampToDateInput(detail.createdAt)
  const photoDates = detail.photos
    .map(photo => timestampToDateInput(photo.createdAt))
    .filter(Boolean)
    .sort()

  const displayCreatedDate = detail.displayCreatedDate || fallbackCreatedDate
  const photoDateStart = detail.photoDateStart || photoDates[0] || ''
  const photoDateEnd = detail.photoDateEnd || photoDates.at(-1) || ''

  dateDraftAlbumId.value = detail.id
  albumDateDraft.displayCreatedDate = displayCreatedDate
  albumDateDraft.photoDateStart = photoDateStart
  albumDateDraft.photoDateEnd = photoDateEnd
  savedAlbumDateDraft.displayCreatedDate = displayCreatedDate
  savedAlbumDateDraft.photoDateStart = photoDateStart
  savedAlbumDateDraft.photoDateEnd = photoDateEnd
}

const applyAlbumDescriptionDraft = (detail: AlbumDetail, force = false) => {
  if (!force && descriptionDraftAlbumId.value === detail.id && albumDescriptionDirty.value) return
  descriptionDraftAlbumId.value = detail.id
  albumDescriptionDraft.value = detail.description || ''
  savedAlbumDescriptionDraft.value = detail.description || ''
}

const resetAlbumDateDraft = () => {
  albumDateDraft.displayCreatedDate = savedAlbumDateDraft.displayCreatedDate
  albumDateDraft.photoDateStart = savedAlbumDateDraft.photoDateStart
  albumDateDraft.photoDateEnd = savedAlbumDateDraft.photoDateEnd
}

const resetAlbumDescriptionDraft = () => {
  albumDescriptionDraft.value = savedAlbumDescriptionDraft.value
}

const selectAlbum = (albumId: string) => {
  if (albumId === selectedAlbumId.value) return
  if (isAlbumInteractionLocked.value) return
  if (albumMetadataDirty.value) {
    toast.add({
      title: '相簿资料尚未保存',
      description: '请先保存简介或日期，或者放弃修改后再切换相簿。',
      color: 'warning',
    })
    return
  }
  selectedAlbumId.value = albumId
}

const loadAlbumDetail = async (albumId: string): Promise<boolean> => {
  const requestSerial = ++detailRequestSerial
  photos.value = []
  photoError.value = ''
  isAlbumDetailReady.value = false

  if (!albumId) return true
  isLoadingPhotos.value = true

  try {
    const detail = await adminFetch<AlbumDetail>(`/api/albums/${albumId}`)
    if (requestSerial !== detailRequestSerial || selectedAlbumId.value !== albumId) return false

    photos.value = detail.photos
    applyAlbumDateDraft(detail)
    applyAlbumDescriptionDraft(detail)
    const albumIndex = albums.value.findIndex(album => album.id === albumId)
    if (albumIndex >= 0) {
      albums.value[albumIndex] = {
        id: detail.id,
        name: detail.name,
        description: detail.description,
        createdAt: detail.createdAt,
        displayCreatedDate: detail.displayCreatedDate,
        photoDateStart: detail.photoDateStart,
        photoDateEnd: detail.photoDateEnd,
        position: detail.position,
        photoCount: detail.photoCount,
      }
    }
    isAlbumDetailReady.value = true
    return true
  } catch (error) {
    if (requestSerial === detailRequestSerial) {
      photoError.value = getAdminApiErrorMessage(error)
    }
    return false
  } finally {
    if (requestSerial === detailRequestSerial) {
      isLoadingPhotos.value = false
    }
  }
}

const refreshAlbums = async (preferredAlbumId?: string): Promise<boolean> => {
  if (isLoadingAlbums.value) return false
  isLoadingAlbums.value = true
  albumError.value = ''

  try {
    const nextAlbums = await adminFetch<Album[]>('/api/albums')
    albums.value = nextAlbums
    const existingAlbumIds = new Set(nextAlbums.map(album => album.id))
    exportAlbumIds.value = exportAlbumIds.value.filter(albumId => existingAlbumIds.has(albumId))

    const candidateId = preferredAlbumId || selectedAlbumId.value
    const nextSelectedId = nextAlbums.some(album => album.id === candidateId)
      ? candidateId
      : (nextAlbums[0]?.id || '')

    if (nextSelectedId === selectedAlbumId.value) {
      return await loadAlbumDetail(nextSelectedId)
    } else {
      selectedAlbumId.value = nextSelectedId
      return true
    }
  } catch (error) {
    albumError.value = getAdminApiErrorMessage(error)
    return false
  } finally {
    isLoadingAlbums.value = false
  }
}

const createAlbum = async () => {
  if (isAlbumInteractionLocked.value) return
  if (albumMetadataDirty.value) {
    toast.add({ title: '请先保存或放弃当前相簿的资料修改', color: 'warning' })
    return
  }
  const name = newAlbumName.value.trim()
  if (!name) {
    toast.add({ title: '请输入相簿名称', color: 'warning' })
    return
  }

  isCreating.value = true
  try {
    const created = await adminFetch<Album>('/api/albums', {
      method: 'POST',
      body: { name, description: newAlbumDescription.value },
    })
    newAlbumName.value = ''
    newAlbumDescription.value = ''
    isCreateDialogOpen.value = false
    albums.value.unshift(created)
    selectedAlbumId.value = created.id
    toast.add({ title: '相簿已创建', description: created.name, color: 'success' })
  } catch (error) {
    toast.add({
      title: '创建相簿失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isCreating.value = false
  }
}

const saveAlbumDescription = async () => {
  if (isAlbumInteractionLocked.value || !isAlbumDetailReady.value) return
  if (!albumDescriptionDirty.value || !selectedAlbumId.value) return
  const description = albumDescriptionDraft.value.trim()
  if (description.length > 1000) {
    toast.add({ title: '相簿简介不能超过 1000 个字符', color: 'warning' })
    return
  }

  isSavingAlbumDescription.value = true
  try {
    const updated = await adminFetch<Album>(`/api/albums/${selectedAlbumId.value}`, {
      method: 'PATCH',
      body: { description },
    })
    const albumIndex = albums.value.findIndex(album => album.id === updated.id)
    if (albumIndex >= 0) albums.value[albumIndex] = updated
    if (selectedAlbumId.value === updated.id) {
      applyAlbumDescriptionDraft({ ...updated, photos: photos.value }, true)
    }
    toast.add({
      title: '相簿简介已保存',
      description: '相簿列表和相簿详情页都会显示这段简介。',
      color: 'success',
    })
  } catch (error) {
    toast.add({
      title: '保存相簿简介失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isSavingAlbumDescription.value = false
  }
}

const moveAlbum = async (albumId: string, delta: -1 | 1) => {
  if (isAlbumInteractionLocked.value) return
  if (albumMetadataDirty.value) {
    toast.add({ title: '请先保存或放弃当前相簿的资料修改', color: 'warning' })
    return
  }
  const currentIndex = albums.value.findIndex(album => album.id === albumId)
  const nextIndex = currentIndex + delta
  if (currentIndex < 0 || nextIndex < 0 || nextIndex >= albums.value.length) return
  const ordered = [...albums.value]
  const [moved] = ordered.splice(currentIndex, 1)
  if (!moved) return
  ordered.splice(nextIndex, 0, moved)

  isReorderingAlbums.value = true
  try {
    albums.value = await adminFetch<Album[]>('/api/albums/order', {
      method: 'POST',
      body: { albumIds: ordered.map(album => album.id) },
    })
    toast.add({ title: '相簿顺序已更新', color: 'success' })
  } catch (error) {
    toast.add({
      title: '调整相簿顺序失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isReorderingAlbums.value = false
  }
}

const saveAlbumDates = async () => {
  if (isAlbumInteractionLocked.value || !isAlbumDetailReady.value) return
  if (!albumDatesDirty.value) return
  const displayCreatedDate = albumDateDraft.displayCreatedDate.trim()
  const photoDateStart = albumDateDraft.photoDateStart.trim()
  const photoDateEnd = albumDateDraft.photoDateEnd.trim()

  if (!displayCreatedDate || !photoDateStart || !photoDateEnd) {
    toast.add({ title: '请完整填写三个日期', color: 'warning' })
    return
  }
  if (photoDateStart > photoDateEnd) {
    toast.add({ title: '图片日期范围无效', description: '开始日期不能晚于结束日期。', color: 'warning' })
    return
  }
  if (!selectedAlbumId.value) return

  isSavingAlbumDates.value = true
  try {
    const updated = await adminFetch<Album>(`/api/albums/${selectedAlbumId.value}`, {
      method: 'PATCH',
      body: { displayCreatedDate, photoDateStart, photoDateEnd },
    })
    const albumIndex = albums.value.findIndex(album => album.id === updated.id)
    if (albumIndex >= 0) albums.value[albumIndex] = updated
    if (selectedAlbumId.value === updated.id) {
      applyAlbumDateDraft({ ...updated, photos: photos.value }, true)
    }
    toast.add({
      title: '相簿日期已保存',
      description: '公开相簿页会使用管理员指定的日期。',
      color: 'success',
    })
  } catch (error) {
    toast.add({
      title: '保存相簿日期失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isSavingAlbumDates.value = false
  }
}

const clearAlbumDates = async () => {
  if (isAlbumInteractionLocked.value || !isAlbumDetailReady.value) return
  if (!selectedAlbumId.value) return
  if (!albumHasCustomDates.value) {
    resetAlbumDateDraft()
    toast.add({ title: '已恢复当前自动日期', color: 'success' })
    return
  }

  isSavingAlbumDates.value = true
  try {
    const updated = await adminFetch<Album>(`/api/albums/${selectedAlbumId.value}`, {
      method: 'PATCH',
      body: { displayCreatedDate: null, photoDateStart: null, photoDateEnd: null },
    })
    const albumIndex = albums.value.findIndex(album => album.id === updated.id)
    if (albumIndex >= 0) albums.value[albumIndex] = updated
    if (selectedAlbumId.value === updated.id) {
      applyAlbumDateDraft({ ...updated, photos: photos.value }, true)
    }
    toast.add({
      title: '已恢复自动日期',
      description: '公开页面重新根据相簿创建记录和当前图片记录显示日期。',
      color: 'success',
    })
  } catch (error) {
    toast.add({
      title: '恢复自动日期失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isSavingAlbumDates.value = false
  }
}

const handleFileSelection = (event: Event) => {
  const input = event.target as HTMLInputElement
  selectedFiles.value = Array.from(input.files || [])
}

const clearSelectedFiles = () => {
  selectedFiles.value = []
  if (uploadInput.value) uploadInput.value.value = ''
}

const uploadPhotos = async () => {
  if (isAlbumInteractionLocked.value || !isAlbumDetailReady.value) return
  if (albumMetadataDirty.value) {
    toast.add({ title: '请先保存或放弃相簿资料修改，再上传图片', color: 'warning' })
    return
  }
  if (!selectedAlbumId.value) {
    toast.add({ title: '请先创建并选中相簿', color: 'warning' })
    return
  }
  if (!selectedFiles.value.length) {
    toast.add({ title: '请选择要上传的图片', color: 'warning' })
    return
  }

  const formData = new FormData()
  for (const file of selectedFiles.value) formData.append('files', file, file.name)

  const albumId = selectedAlbumId.value
  isUploading.value = true
  isAlbumDetailReady.value = false
  try {
    const uploaded = await adminFetch<Photo[]>(
      `/api/albums/${albumId}/photos`,
      { method: 'POST', body: formData },
    )
    clearSelectedFiles()
    const refreshed = await refreshAlbums(albumId)
    if (!refreshed) {
      toast.add({
        title: '图片已上传，但页面刷新失败',
        description: albumError.value || photoError.value || '请点击刷新，确认相簿的最新状态。',
        color: 'warning',
      })
      return
    }
    toast.add({
      title: '上传完成',
      description: `已将 ${uploaded.length} 张图片写入「${selectedAlbum.value?.name || '相簿'}」`,
      color: 'success',
    })
  } catch (error) {
    const uploadError = getAdminApiErrorMessage(error)
    clearSelectedFiles()
    const refreshed = await refreshAlbums(albumId)
    toast.add({
      title: refreshed ? '上传请求未确认' : '上传状态无法确认',
      description: refreshed
        ? `${uploadError}；已重新同步相簿，请检查图片列表后再决定是否重试。`
        : `${uploadError}；请先刷新相簿，确认最新状态后再重试。`,
      color: 'warning',
    })
  } finally {
    isUploading.value = false
  }
}

watch(selectedAlbumId, (albumId) => {
  clearSelectedFiles()
  isAlbumDetailReady.value = false
  dateDraftAlbumId.value = albumId
  albumDateDraft.displayCreatedDate = ''
  albumDateDraft.photoDateStart = ''
  albumDateDraft.photoDateEnd = ''
  savedAlbumDateDraft.displayCreatedDate = ''
  savedAlbumDateDraft.photoDateStart = ''
  savedAlbumDateDraft.photoDateEnd = ''
  descriptionDraftAlbumId.value = albumId
  albumDescriptionDraft.value = ''
  savedAlbumDescriptionDraft.value = ''
  void loadAlbumDetail(albumId)
})

const confirmDiscardAlbumMetadata = () =>
  !albumMetadataDirty.value || window.confirm('相簿简介或显示日期尚未保存，确定要放弃修改吗？')

const handleBeforeUnload = (event: BeforeUnloadEvent) => {
  if (!albumMetadataDirty.value) return
  event.preventDefault()
  event.returnValue = true
}

onBeforeRouteLeave(() => confirmDiscardAlbumMetadata())
onMounted(() => {
  window.addEventListener('beforeunload', handleBeforeUnload)
  void refreshAlbums()
})
onBeforeUnmount(() => window.removeEventListener('beforeunload', handleBeforeUnload))
</script>

<template>
  <UDashboardPanel :ui="{ body: 'p-0 sm:p-0' }">
    <template #header>
      <UDashboardNavbar title="相簿工作台">
        <template #right>
          <UButton icon="tabler:refresh" color="neutral" variant="ghost" :loading="isLoadingAlbums" :disabled="isAlbumInteractionLocked" @click="refreshAlbums()">
            刷新
          </UButton>
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="dashboard-panel-body space-y-6">
        <DashboardPageHero
          eyebrow="Album workspace"
          title="先选相簿，再完成一件事"
          description="图片必须放进相簿空间。选择一个相簿后，可在同一个工作区上传、编辑公开资料或打包下载。"
          icon="tabler:album"
        >
          <template #actions>
            <UButton icon="tabler:plus" size="lg" @click="openCreateDialog">新建相簿</UButton>
            <UButton color="neutral" variant="soft" icon="tabler:file-zip" size="lg" :disabled="!albums.length" @click="openExportWorkspace()">批量打包</UButton>
          </template>
        </DashboardPageHero>

        <UAlert v-if="albumError" color="error" variant="subtle" icon="tabler:alert-circle" title="相簿加载失败" :description="albumError" />

        <div class="grid min-h-[680px] grid-cols-1 gap-5 lg:grid-cols-[290px_minmax(0,1fr)]">
          <aside class="dashboard-section flex min-h-0 flex-col overflow-hidden lg:sticky lg:top-4 lg:max-h-[calc(100svh-7rem)]">
            <div class="border-b border-default p-4">
              <div class="flex items-center justify-between gap-3">
                <div>
                  <h2 class="font-semibold text-highlighted">我的相簿</h2>
                  <p class="mt-0.5 text-xs text-muted">{{ albums.length }} 个空间</p>
                </div>
                <UButton :color="isOrderMode ? 'primary' : 'neutral'" :variant="isOrderMode ? 'soft' : 'ghost'" size="sm" icon="tabler:sort-ascending" :disabled="isAlbumInteractionLocked || albumMetadataDirty" @click="isOrderMode = !isOrderMode; albumQuery = ''">
                  {{ isOrderMode ? '完成' : '排序' }}
                </UButton>
              </div>
              <UInput v-if="!isOrderMode" v-model="albumQuery" icon="tabler:search" placeholder="搜索相簿" class="mt-3 w-full" />
              <p v-else class="mt-3 rounded-lg bg-primary/10 px-3 py-2 text-xs leading-5 text-primary">用箭头调整公开页面中的前后顺序。</p>
            </div>

            <div v-if="isLoadingAlbums && !albums.length" class="space-y-2 p-3">
              <USkeleton v-for="index in 5" :key="index" class="h-16 w-full rounded-xl" />
            </div>

            <div v-else-if="filteredAlbums.length" class="custom-scrollbar min-h-0 flex-1 space-y-1 overflow-y-auto p-2">
              <div
                v-for="album in filteredAlbums"
                :key="album.id"
                class="group flex items-center rounded-xl border transition"
                :class="selectedAlbumId === album.id ? 'border-primary/20 bg-primary/10 shadow-sm' : 'border-transparent hover:bg-elevated'"
              >
                <button type="button" class="flex min-w-0 flex-1 items-center gap-3 px-3 py-3 text-left" :disabled="isAlbumInteractionLocked || isOrderMode" @click="selectAlbum(album.id)">
                  <span class="flex size-10 shrink-0 items-center justify-center rounded-xl" :class="selectedAlbumId === album.id ? 'bg-primary text-inverted' : 'bg-elevated text-muted'">
                    <Icon name="tabler:photo" class="size-5" />
                  </span>
                  <span class="min-w-0 flex-1">
                    <span class="block truncate text-sm font-medium text-highlighted">{{ album.name }}</span>
                    <span class="mt-0.5 block text-xs text-muted">{{ album.photoCount }} 张图片</span>
                  </span>
                </button>
                <div v-if="isOrderMode" class="flex shrink-0 items-center pr-2">
                  <UButton icon="tabler:arrow-up" color="neutral" variant="ghost" size="xs" aria-label="向前移动" :disabled="albums[0]?.id === album.id || isAlbumInteractionLocked" @click="moveAlbum(album.id, -1)" />
                  <UButton icon="tabler:arrow-down" color="neutral" variant="ghost" size="xs" aria-label="向后移动" :disabled="albums.at(-1)?.id === album.id || isAlbumInteractionLocked" @click="moveAlbum(album.id, 1)" />
                </div>
                <Icon v-else-if="selectedAlbumId === album.id" name="tabler:chevron-right" class="mr-3 size-4 shrink-0 text-primary" />
              </div>
            </div>

            <div v-else class="flex min-h-56 flex-1 flex-col items-center justify-center px-5 text-center">
              <span class="flex size-12 items-center justify-center rounded-2xl bg-elevated text-muted"><Icon :name="albums.length ? 'tabler:search-off' : 'tabler:folder-plus'" class="size-6" /></span>
              <p class="mt-3 font-medium text-highlighted">{{ albums.length ? '没有匹配的相簿' : '先创建一个相簿' }}</p>
              <p class="mt-1 text-sm text-muted">{{ albums.length ? '换个关键词试试。' : '创建空间后才可以上传图片。' }}</p>
            </div>

            <div class="border-t border-default p-3">
              <UButton block color="neutral" variant="soft" icon="tabler:plus" :disabled="hasActiveMutation" @click="openCreateDialog">新建相簿</UButton>
            </div>
          </aside>

          <main class="min-w-0">
            <section v-if="selectedAlbum" class="dashboard-section overflow-hidden">
              <header class="border-b border-default px-4 pt-5 sm:px-6">
                <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
                  <div class="flex min-w-0 items-start gap-3">
                    <span class="flex size-11 shrink-0 items-center justify-center rounded-2xl bg-primary/10 text-primary"><Icon name="tabler:album" class="size-6" /></span>
                    <div class="min-w-0">
                      <div class="flex flex-wrap items-center gap-2">
                        <h2 class="truncate text-xl font-semibold text-highlighted">{{ selectedAlbum.name }}</h2>
                        <UBadge color="success" variant="soft">当前空间</UBadge>
                      </div>
                      <p class="mt-1 text-sm text-muted">{{ selectedAlbum.photoCount }} 张图片 · 创建于 {{ selectedAlbum.displayCreatedDate || timestampToDateInput(selectedAlbum.createdAt) }}</p>
                    </div>
                  </div>
                  <div class="flex shrink-0 flex-wrap gap-2">
                    <UButton color="neutral" variant="soft" icon="tabler:file-zip" @click="openExportWorkspace(true)">打包此相簿</UButton>
                    <UButton icon="tabler:upload" @click="activeWorkspaceTab = 'photos'; uploadInput?.click()">选择图片</UButton>
                  </div>
                </div>

                <nav class="custom-scrollbar mt-5 flex gap-1 overflow-x-auto" aria-label="相簿操作">
                  <button
                    v-for="tab in workspaceTabs"
                    :key="tab.value"
                    type="button"
                    class="flex shrink-0 items-center gap-2 border-b-2 px-3 py-3 text-sm font-medium transition"
                    :class="activeWorkspaceTab === tab.value ? 'border-primary text-primary' : 'border-transparent text-muted hover:text-highlighted'"
                    @click="activeWorkspaceTab = tab.value"
                  >
                    <Icon :name="tab.icon" class="size-4" />{{ tab.label }}
                    <span v-if="tab.value === 'photos'" class="rounded-full bg-elevated px-1.5 py-0.5 text-[10px]">{{ photos.length }}</span>
                    <span v-if="tab.value === 'export' && exportAlbumIds.length" class="rounded-full bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">{{ exportAlbumIds.length }}</span>
                  </button>
                </nav>
              </header>

              <div v-if="activeWorkspaceTab === 'photos'" class="space-y-6 p-4 sm:p-6">
                <section class="rounded-2xl border border-dashed border-primary/30 bg-primary/5 p-4 sm:p-5">
                  <input ref="uploadInput" type="file" multiple accept=".png,.jpg,.jpeg,.webp,image/png,image/jpeg,image/webp" class="hidden" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked" @change="handleFileSelection" />
                  <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                    <div class="flex items-center gap-3">
                      <span class="flex size-11 shrink-0 items-center justify-center rounded-xl bg-primary text-inverted"><Icon name="tabler:cloud-upload" class="size-6" /></span>
                      <div>
                        <p class="font-medium text-highlighted">上传到「{{ selectedAlbum.name }}」</p>
                        <p class="mt-1 text-xs leading-5 text-muted">PNG、JPG/JPEG、WEBP · 最多 100 张 · 总计 384 MB</p>
                      </div>
                    </div>
                    <UButton color="neutral" variant="outline" icon="tabler:photo-plus" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked" @click="uploadInput?.click()">选择图片</UButton>
                  </div>

                  <div v-if="selectedFiles.length" class="mt-4 flex flex-col gap-3 rounded-xl border border-primary/15 bg-default p-3 sm:flex-row sm:items-center sm:justify-between">
                    <div class="flex items-center gap-2 text-sm"><Icon name="tabler:files" class="size-4 text-primary" /><span>已选择 <strong>{{ selectedFiles.length }}</strong> 张，共 {{ formatBytes(selectedBytes) }}</span></div>
                    <div class="flex gap-2">
                      <UButton size="sm" color="neutral" variant="ghost" :disabled="hasActiveMutation" @click="clearSelectedFiles">清空</UButton>
                      <UButton size="sm" icon="tabler:upload" :loading="isUploading" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked || albumMetadataDirty" @click="uploadPhotos">开始上传</UButton>
                    </div>
                  </div>
                </section>

                <UAlert v-if="photoError" color="error" variant="subtle" icon="tabler:alert-circle" title="相簿内容加载失败" :description="photoError" />

                <section>
                  <div class="mb-4 flex items-center justify-between gap-3">
                    <div><h3 class="font-semibold text-highlighted">相簿内容</h3><p class="mt-1 text-sm text-muted">点击图片可在新窗口查看原文件</p></div>
                    <UBadge color="neutral" variant="soft">{{ photos.length }} 张</UBadge>
                  </div>
                  <div v-if="isLoadingPhotos" class="grid grid-cols-2 gap-3 sm:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5">
                    <USkeleton v-for="index in 10" :key="index" class="aspect-square w-full rounded-xl" />
                  </div>
                  <div v-else-if="photos.length" class="grid grid-cols-2 gap-3 sm:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5">
                    <a v-for="photo in photos" :key="photo.id" :href="`/api/photos/${photo.id}/file`" target="_blank" rel="noopener noreferrer" class="group min-w-0 overflow-hidden rounded-xl border border-default bg-elevated transition hover:-translate-y-0.5 hover:border-primary/30 hover:shadow-md">
                      <div class="aspect-square overflow-hidden bg-muted"><img :src="`/api/photos/${photo.id}/thumbnail`" :alt="photo.originalName" loading="lazy" class="size-full object-cover transition duration-300 group-hover:scale-105" /></div>
                      <div class="p-2.5"><p class="truncate text-sm font-medium text-highlighted" :title="photo.originalName">{{ photo.originalName }}</p><p class="mt-1 flex items-center justify-between gap-2 text-xs text-muted"><span>{{ photo.format.toUpperCase() }}</span><span>{{ formatBytes(photo.byteSize) }}</span></p></div>
                    </a>
                  </div>
                  <div v-else class="flex min-h-64 flex-col items-center justify-center rounded-2xl border border-dashed border-default text-center">
                    <span class="flex size-12 items-center justify-center rounded-2xl bg-elevated text-muted"><Icon name="tabler:photo-plus" class="size-6" /></span>
                    <p class="mt-3 font-medium text-highlighted">这个相簿还是空的</p><p class="mt-1 text-sm text-muted">从上方选择图片，确认后再开始上传。</p>
                  </div>
                </section>
              </div>

              <div v-else-if="activeWorkspaceTab === 'details'" class="divide-y divide-default">
                <section class="p-4 sm:p-6">
                  <div class="grid gap-6 xl:grid-cols-[220px_minmax(0,1fr)]">
                    <div><span class="flex size-10 items-center justify-center rounded-xl bg-info/10 text-info"><Icon name="tabler:align-left" class="size-5" /></span><h3 class="mt-3 font-semibold text-highlighted">公开简介</h3><p class="mt-1 text-sm leading-6 text-muted">显示在相簿列表和详情页，留空保存即可隐藏。</p></div>
                    <form class="space-y-3" @submit.prevent="saveAlbumDescription">
                      <UTextarea v-model="albumDescriptionDraft" :rows="6" maxlength="1000" placeholder="介绍这个相簿的主题、地点或故事" class="w-full" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked" />
                      <div class="flex flex-wrap items-center justify-between gap-3"><span class="text-xs text-muted">{{ albumDescriptionDraft.length }} / 1000</span><div class="flex gap-2"><UButton v-if="albumDescriptionDirty" type="button" color="neutral" variant="ghost" icon="tabler:arrow-back-up" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked" @click="resetAlbumDescriptionDraft">放弃</UButton><UButton type="submit" icon="tabler:device-floppy" :loading="isSavingAlbumDescription" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked || !albumDescriptionDirty">保存简介</UButton></div></div>
                    </form>
                  </div>
                </section>

                <section class="p-4 sm:p-6">
                  <div class="grid gap-6 xl:grid-cols-[220px_minmax(0,1fr)]">
                    <div><span class="flex size-10 items-center justify-center rounded-xl bg-warning/10 text-warning"><Icon name="tabler:calendar-event" class="size-5" /></span><div class="mt-3 flex flex-wrap items-center gap-2"><h3 class="font-semibold text-highlighted">公开显示日期</h3><UBadge :color="albumHasCustomDates ? 'primary' : 'neutral'" variant="soft">{{ albumHasCustomDates ? '手动' : '自动' }}</UBadge></div><p class="mt-1 text-sm leading-6 text-muted">只改变公开页面的文字，不修改图片时间戳和文件。</p></div>
                    <form class="space-y-4" @submit.prevent="saveAlbumDates">
                      <UAlert v-if="photoError && !isAlbumDetailReady" color="error" variant="subtle" icon="tabler:alert-circle" title="相簿详情加载失败" description="日期表单已锁定，请刷新后重试。" />
                      <div class="grid grid-cols-1 gap-4 md:grid-cols-3">
                        <UFormField label="相簿创建日期" required><UInput v-model="albumDateDraft.displayCreatedDate" type="date" icon="tabler:clock-plus" class="w-full" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked" /></UFormField>
                        <UFormField label="图片范围开始" required><UInput v-model="albumDateDraft.photoDateStart" type="date" icon="tabler:calendar" class="w-full" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked" /></UFormField>
                        <UFormField label="图片范围结束" required><UInput v-model="albumDateDraft.photoDateEnd" type="date" icon="tabler:calendar" class="w-full" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked" /></UFormField>
                      </div>
                      <div class="flex flex-wrap justify-end gap-2"><UButton v-if="albumDatesDirty" type="button" color="neutral" variant="ghost" icon="tabler:arrow-back-up" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked" @click="resetAlbumDateDraft">放弃</UButton><UButton type="button" color="neutral" variant="soft" icon="tabler:restore" :loading="isSavingAlbumDates && albumHasCustomDates" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked || (!albumHasCustomDates && !albumDatesDirty)" @click="clearAlbumDates">恢复自动日期</UButton><UButton type="submit" icon="tabler:device-floppy" :loading="isSavingAlbumDates" :disabled="!isAlbumDetailReady || isAlbumInteractionLocked || !albumDatesDirty">保存日期</UButton></div>
                    </form>
                  </div>
                </section>
              </div>

              <div v-else class="p-4 sm:p-6">
                <div class="mx-auto max-w-4xl">
                  <div class="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
                    <div><h3 class="text-lg font-semibold text-highlighted">选择要打包的相簿</h3><p class="mt-1 text-sm text-muted">单选生成一个 ZIP；多选生成外层 ZIP，里面每个相簿各有一个 ZIP。</p></div>
                    <div class="flex gap-2"><UButton size="sm" color="neutral" variant="soft" :disabled="exportAlbumIds.length === albums.length" @click="exportAlbumIds = albums.map(album => album.id)">全选</UButton><UButton size="sm" color="neutral" variant="ghost" :disabled="!exportAlbumIds.length" @click="exportAlbumIds = []">清空</UButton></div>
                  </div>

                  <div class="mt-5 grid gap-2 sm:grid-cols-2">
                    <label v-for="album in albums" :key="`export-${album.id}`" class="flex cursor-pointer items-center gap-3 rounded-xl border p-3 transition" :class="exportAlbumIds.includes(album.id) ? 'border-primary/30 bg-primary/10' : 'border-default hover:bg-elevated'">
                      <input type="checkbox" class="size-4 accent-primary" :checked="exportAlbumIds.includes(album.id)" @change="handleExportAlbumToggle(album.id, $event)">
                      <span class="flex size-9 shrink-0 items-center justify-center rounded-lg bg-elevated text-muted"><Icon name="tabler:album" class="size-4" /></span>
                      <span class="min-w-0 flex-1"><span class="block truncate text-sm font-medium text-highlighted">{{ album.name }}</span><span class="mt-0.5 block text-xs text-muted">{{ album.photoCount }} 张图片</span></span>
                    </label>
                  </div>

                  <div class="mt-6 flex flex-col gap-4 rounded-2xl bg-elevated p-4 sm:flex-row sm:items-center sm:justify-between">
                    <div class="flex items-center gap-3"><span class="flex size-10 items-center justify-center rounded-xl bg-primary/10 text-primary"><Icon name="tabler:file-zip" class="size-5" /></span><div><p class="font-medium text-highlighted">{{ exportAlbumIds.length ? `已选择 ${exportAlbumIds.length} 个相簿` : '尚未选择相簿' }}</p><p class="mt-0.5 text-xs text-muted">{{ exportAlbumIds.length === 1 ? '图片会直接放在 ZIP 内。' : exportAlbumIds.length > 1 ? `将生成包含 ${exportAlbumIds.length} 个相簿 ZIP 的压缩包。` : '勾选一个或多个相簿后开始。' }}</p></div></div>
                    <UButton icon="tabler:download" :loading="isStartingExport" :disabled="!exportAlbumIds.length" @click="startAlbumExport">开始打包下载</UButton>
                  </div>
                </div>
              </div>
            </section>

            <section v-else class="dashboard-section flex min-h-[520px] flex-col items-center justify-center px-6 text-center">
              <span class="flex size-16 items-center justify-center rounded-3xl bg-primary/10 text-primary"><Icon name="tabler:folder-plus" class="size-8" /></span>
              <h2 class="mt-5 text-xl font-semibold text-highlighted">从第一个相簿开始</h2>
              <p class="mt-2 max-w-md text-sm leading-6 text-muted">ChronoFrame 只允许向相簿空间上传图片。创建后，这里会变成该相簿的完整工作区。</p>
              <UButton class="mt-5" size="lg" icon="tabler:plus" @click="openCreateDialog">创建相簿</UButton>
            </section>
          </main>
        </div>
      </div>
    </template>
  </UDashboardPanel>

  <Teleport to="body">
    <Transition enter-active-class="transition duration-200" enter-from-class="opacity-0" leave-active-class="transition duration-150" leave-to-class="opacity-0">
      <div v-if="isCreateDialogOpen" class="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 p-4 backdrop-blur-sm" role="presentation" @click.self="closeCreateDialog">
        <section role="dialog" aria-modal="true" aria-labelledby="create-album-title" class="w-full max-w-lg overflow-hidden rounded-2xl border border-default bg-default shadow-2xl">
          <header class="flex items-start justify-between gap-4 border-b border-default px-5 py-4">
            <div class="flex items-center gap-3"><span class="flex size-10 items-center justify-center rounded-xl bg-primary/10 text-primary"><Icon name="tabler:folder-plus" class="size-5" /></span><div><h2 id="create-album-title" class="font-semibold text-highlighted">新建相簿空间</h2><p class="mt-0.5 text-sm text-muted">创建后会自动选中，可立即上传图片。</p></div></div>
            <UButton icon="tabler:x" color="neutral" variant="ghost" aria-label="关闭" :disabled="isCreating" @click="closeCreateDialog" />
          </header>
          <form class="space-y-5 p-5" @submit.prevent="createAlbum">
            <UFormField label="相簿名称" description="最多 100 个字符" required><UInput v-model="newAlbumName" maxlength="100" placeholder="例如：2026 夏日旅行" icon="tabler:album" class="w-full" :disabled="isCreating" autofocus /></UFormField>
            <UFormField label="相簿简介" description="可稍后在资料页修改；留空也可以创建。"><UTextarea v-model="newAlbumDescription" :rows="4" maxlength="1000" placeholder="记录这个相簿的主题、地点或故事" class="w-full" :disabled="isCreating" /></UFormField>
            <div class="flex justify-end gap-2"><UButton type="button" color="neutral" variant="ghost" :disabled="isCreating" @click="closeCreateDialog">取消</UButton><UButton type="submit" icon="tabler:plus" :loading="isCreating" :disabled="!newAlbumName.trim()">创建并进入</UButton></div>
          </form>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>
