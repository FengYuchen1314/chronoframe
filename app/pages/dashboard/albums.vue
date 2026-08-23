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
const newAlbumName = ref('')
const selectedFiles = ref<File[]>([])
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

const isLoadingAlbums = ref(false)
const isLoadingPhotos = ref(false)
const isCreating = ref(false)
const isUploading = ref(false)
const isSavingAlbumDates = ref(false)
const isAlbumDetailReady = ref(false)
const albumError = ref('')
const photoError = ref('')
let detailRequestSerial = 0

const selectedAlbum = computed(() =>
  albums.value.find(album => album.id === selectedAlbumId.value) || null,
)
const albumDatesDirty = computed(() =>
  dateDraftAlbumId.value === selectedAlbumId.value
  && (
    albumDateDraft.displayCreatedDate !== savedAlbumDateDraft.displayCreatedDate
    || albumDateDraft.photoDateStart !== savedAlbumDateDraft.photoDateStart
    || albumDateDraft.photoDateEnd !== savedAlbumDateDraft.photoDateEnd
  ),
)
const albumHasCustomDates = computed(() => Boolean(
  selectedAlbum.value?.displayCreatedDate
  || selectedAlbum.value?.photoDateStart
  || selectedAlbum.value?.photoDateEnd,
))
const hasActiveMutation = computed(() =>
  isCreating.value || isUploading.value || isSavingAlbumDates.value,
)
const isAlbumInteractionLocked = computed(() =>
  isLoadingAlbums.value || isLoadingPhotos.value || hasActiveMutation.value,
)

const selectedBytes = computed(() =>
  selectedFiles.value.reduce((total, file) => total + file.size, 0),
)

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

const resetAlbumDateDraft = () => {
  albumDateDraft.displayCreatedDate = savedAlbumDateDraft.displayCreatedDate
  albumDateDraft.photoDateStart = savedAlbumDateDraft.photoDateStart
  albumDateDraft.photoDateEnd = savedAlbumDateDraft.photoDateEnd
}

const selectAlbum = (albumId: string) => {
  if (albumId === selectedAlbumId.value) return
  if (isAlbumInteractionLocked.value) return
  if (albumDatesDirty.value) {
    toast.add({
      title: '日期修改尚未保存',
      description: '请先保存，或点击“放弃修改”后再切换相簿。',
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
    const albumIndex = albums.value.findIndex(album => album.id === albumId)
    if (albumIndex >= 0) {
      albums.value[albumIndex] = {
        id: detail.id,
        name: detail.name,
        createdAt: detail.createdAt,
        displayCreatedDate: detail.displayCreatedDate,
        photoDateStart: detail.photoDateStart,
        photoDateEnd: detail.photoDateEnd,
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
  if (albumDatesDirty.value) {
    toast.add({ title: '请先保存或放弃当前相簿的日期修改', color: 'warning' })
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
      body: { name },
    })
    newAlbumName.value = ''
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
  if (albumDatesDirty.value) {
    toast.add({ title: '请先保存或放弃日期修改，再上传图片', color: 'warning' })
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
  void loadAlbumDetail(albumId)
})

const confirmDiscardAlbumDates = () =>
  !albumDatesDirty.value || window.confirm('相簿显示日期尚未保存，确定要放弃修改吗？')

const handleBeforeUnload = (event: BeforeUnloadEvent) => {
  if (!albumDatesDirty.value) return
  event.preventDefault()
  event.returnValue = true
}

onBeforeRouteLeave(() => confirmDiscardAlbumDates())
onMounted(() => {
  window.addEventListener('beforeunload', handleBeforeUnload)
  void refreshAlbums()
})
onBeforeUnmount(() => window.removeEventListener('beforeunload', handleBeforeUnload))
</script>

<template>
  <UDashboardPanel>
    <template #header>
      <UDashboardNavbar title="相簿">
        <template #right>
          <UButton
            icon="tabler:refresh"
            color="neutral"
            variant="ghost"
            :loading="isLoadingAlbums"
            :disabled="isAlbumInteractionLocked"
            @click="refreshAlbums()"
          >
            刷新
          </UButton>
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="space-y-6">
        <UAlert
          color="info"
          variant="subtle"
          icon="tabler:info-circle"
          title="先创建相簿空间，再上传图片"
          description="所有图片都必须归属于一个相簿；后台不提供脱离相簿的全部图片上传入口。"
        />

        <UAlert
          v-if="albumError"
          color="error"
          variant="subtle"
          icon="tabler:alert-circle"
          title="相簿加载失败"
          :description="albumError"
        />

        <div class="grid grid-cols-1 gap-6 lg:grid-cols-[minmax(250px,320px)_minmax(0,1fr)]">
          <div class="space-y-4">
            <UCard>
              <template #header>
                <div>
                  <h2 class="font-semibold">创建相簿空间</h2>
                  <p class="mt-1 text-sm text-muted">名称最长 100 个字符</p>
                </div>
              </template>

              <form class="space-y-3" @submit.prevent="createAlbum">
                <UFormField label="相簿名称" required>
                  <UInput
                    v-model="newAlbumName"
                    maxlength="100"
                    placeholder="例如：2026 夏日旅行"
                    icon="tabler:album"
                    class="w-full"
                    :disabled="hasActiveMutation"
                  />
                </UFormField>
                <UButton type="submit" block icon="tabler:plus" :loading="isCreating" :disabled="albumDatesDirty || isAlbumInteractionLocked">
                  创建并选中
                </UButton>
              </form>
            </UCard>

            <UCard :ui="{ body: 'p-2 sm:p-2' }">
              <template #header>
                <div class="flex items-center justify-between">
                  <h2 class="font-semibold">相簿空间</h2>
                  <UBadge color="neutral" variant="soft">{{ albums.length }}</UBadge>
                </div>
              </template>

              <div v-if="isLoadingAlbums && !albums.length" class="space-y-2 p-2">
                <USkeleton v-for="index in 3" :key="index" class="h-16 w-full" />
              </div>

              <div v-else-if="albums.length" class="max-h-[52vh] space-y-1 overflow-y-auto">
                <button
                  v-for="album in albums"
                  :key="album.id"
                  type="button"
                  class="flex w-full items-center gap-3 rounded-md px-3 py-3 text-left transition"
                  :class="selectedAlbumId === album.id ? 'bg-primary/10 text-primary' : 'hover:bg-elevated'"
                  :disabled="isAlbumInteractionLocked"
                  @click="selectAlbum(album.id)"
                >
                  <Icon name="tabler:album" class="size-5 shrink-0" />
                  <span class="min-w-0 flex-1">
                    <span class="block truncate font-medium">{{ album.name }}</span>
                    <span class="mt-0.5 block text-xs opacity-70">{{ album.photoCount }} 张图片</span>
                  </span>
                  <Icon v-if="selectedAlbumId === album.id" name="tabler:check" class="size-4 shrink-0" />
                </button>
              </div>

              <div v-else class="px-3 py-8 text-center">
                <Icon name="tabler:album-off" class="mx-auto size-8 text-muted" />
                <p class="mt-2 text-sm text-muted">尚未创建相簿</p>
              </div>
            </UCard>
          </div>

          <div class="min-w-0 space-y-4">
            <UCard v-if="selectedAlbum">
              <template #header>
                <div class="flex flex-wrap items-start justify-between gap-3">
                  <div>
                    <h2 class="font-semibold">相簿显示日期</h2>
                    <p class="mt-1 text-sm text-muted">保存时三项一起设为手动日期；恢复时三项一起切回自动日期</p>
                  </div>
                  <UBadge :color="albumHasCustomDates ? 'primary' : 'neutral'" variant="soft">
                    {{ albumHasCustomDates ? '手动指定' : '自动日期' }}
                  </UBadge>
                </div>
              </template>

              <UAlert
                v-if="photoError && !isAlbumDetailReady"
                class="mb-4"
                color="error"
                variant="subtle"
                icon="tabler:alert-circle"
                title="相簿详情加载失败"
                description="日期表单已锁定，请刷新后重试。"
              />

              <form class="space-y-4" @submit.prevent="saveAlbumDates">
                <div class="grid grid-cols-1 gap-4 md:grid-cols-3">
                  <UFormField label="创建日期" required>
                    <UInput
                      v-model="albumDateDraft.displayCreatedDate"
                      type="date"
                      icon="tabler:clock-plus"
                      class="w-full"
                      :disabled="!isAlbumDetailReady || isAlbumInteractionLocked"
                    />
                  </UFormField>
                  <UFormField label="图片日期范围 · 开始" required>
                    <UInput
                      v-model="albumDateDraft.photoDateStart"
                      type="date"
                      icon="tabler:calendar"
                      class="w-full"
                      :disabled="!isAlbumDetailReady || isAlbumInteractionLocked"
                    />
                  </UFormField>
                  <UFormField label="图片日期范围 · 结束" required>
                    <UInput
                      v-model="albumDateDraft.photoDateEnd"
                      type="date"
                      icon="tabler:calendar"
                      class="w-full"
                      :disabled="!isAlbumDetailReady || isAlbumInteractionLocked"
                    />
                  </UFormField>
                </div>

                <div class="flex flex-wrap items-center justify-between gap-3">
                  <p class="text-sm text-muted">这里只改变公开展示；自动图片范围根据当前图片记录生成，不会修改文件、时间戳或相簿排序。</p>
                  <div class="flex flex-wrap justify-end gap-2">
                    <UButton
                      v-if="albumDatesDirty"
                      type="button"
                      color="neutral"
                      variant="ghost"
                      icon="tabler:arrow-back-up"
                      :disabled="!isAlbumDetailReady || isAlbumInteractionLocked"
                      @click="resetAlbumDateDraft"
                    >
                      放弃修改
                    </UButton>
                    <UButton
                      type="button"
                      color="neutral"
                      variant="soft"
                      icon="tabler:restore"
                      :loading="isSavingAlbumDates && albumHasCustomDates"
                      :disabled="!isAlbumDetailReady || isAlbumInteractionLocked || (!albumHasCustomDates && !albumDatesDirty)"
                      @click="clearAlbumDates"
                    >
                      恢复自动日期
                    </UButton>
                    <UButton
                      type="submit"
                      icon="tabler:device-floppy"
                      :loading="isSavingAlbumDates"
                      :disabled="!isAlbumDetailReady || isAlbumInteractionLocked || !albumDatesDirty"
                    >
                      保存显示日期
                    </UButton>
                  </div>
                </div>
              </form>
            </UCard>

            <UCard v-if="selectedAlbum">
              <template #header>
                <div class="flex flex-wrap items-start justify-between gap-3">
                  <div>
                    <h2 class="text-lg font-semibold">{{ selectedAlbum.name }}</h2>
                    <p class="mt-1 text-sm text-muted">
                      公开创建日期 {{ selectedAlbum.displayCreatedDate || timestampToDateInput(selectedAlbum.createdAt) }} · {{ selectedAlbum.photoCount }} 张图片
                    </p>
                  </div>
                  <UBadge color="success" variant="soft">已选中上传空间</UBadge>
                </div>
              </template>

              <div class="space-y-4">
                <UFormField
                  label="选择图片"
                  description="单次最多 100 张、总计最多 384 MB；单张最多 100 MB。"
                  required
                >
                  <input
                    ref="uploadInput"
                    type="file"
                    multiple
                    accept=".png,.jpg,.jpeg,.webp,image/png,image/jpeg,image/webp"
                    class="block w-full rounded-md border border-default bg-default px-3 py-2 text-sm file:mr-3 file:rounded-md file:border-0 file:bg-elevated file:px-3 file:py-1.5 file:text-sm file:font-medium"
                    :disabled="!isAlbumDetailReady || isAlbumInteractionLocked"
                    @change="handleFileSelection"
                  />
                </UFormField>

                <div
                  v-if="selectedFiles.length"
                  class="flex flex-wrap items-center justify-between gap-3 rounded-md bg-elevated px-3 py-2 text-sm"
                >
                  <span>已选 {{ selectedFiles.length }} 张，共 {{ formatBytes(selectedBytes) }}</span>
                  <UButton
                    size="xs"
                    color="neutral"
                    variant="ghost"
                    :disabled="hasActiveMutation"
                    @click="clearSelectedFiles"
                  >
                    清空选择
                  </UButton>
                </div>

                <div class="flex justify-end">
                  <UButton
                    icon="tabler:upload"
                    :loading="isUploading"
                    :disabled="!selectedFiles.length || !isAlbumDetailReady || isAlbumInteractionLocked || albumDatesDirty"
                    @click="uploadPhotos"
                  >
                    上传到当前相簿
                  </UButton>
                </div>
              </div>
            </UCard>

            <UCard v-else>
              <div class="flex min-h-52 flex-col items-center justify-center text-center">
                <Icon name="tabler:folder-plus" class="size-10 text-muted" />
                <p class="mt-3 font-medium">先创建相簿空间</p>
                <p class="mt-1 max-w-sm text-sm text-muted">创建完成后才会开放图片上传入口。</p>
              </div>
            </UCard>

            <UCard v-if="selectedAlbum">
              <template #header>
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <h2 class="font-semibold">相簿内容</h2>
                    <p class="mt-1 text-sm text-muted">只展示当前选中相簿</p>
                  </div>
                  <UBadge color="neutral" variant="soft">{{ photos.length }}</UBadge>
                </div>
              </template>

              <UAlert
                v-if="photoError"
                color="error"
                variant="subtle"
                icon="tabler:alert-circle"
                title="相簿内容加载失败"
                :description="photoError"
              />

              <div v-else-if="isLoadingPhotos" class="grid grid-cols-2 gap-3 sm:grid-cols-3 xl:grid-cols-4">
                <USkeleton v-for="index in 8" :key="index" class="aspect-square w-full" />
              </div>

              <div v-else-if="photos.length" class="grid grid-cols-2 gap-3 sm:grid-cols-3 xl:grid-cols-4">
                <a
                  v-for="photo in photos"
                  :key="photo.id"
                  :href="`/api/photos/${photo.id}/file`"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="group min-w-0 overflow-hidden rounded-lg border border-default bg-elevated"
                >
                  <div class="aspect-square overflow-hidden bg-muted">
                    <img
                      :src="`/api/photos/${photo.id}/thumbnail`"
                      :alt="photo.originalName"
                      loading="lazy"
                      class="size-full object-cover transition duration-300 group-hover:scale-105"
                    />
                  </div>
                  <div class="space-y-1 p-2">
                    <p class="truncate text-sm font-medium" :title="photo.originalName">{{ photo.originalName }}</p>
                    <p class="flex items-center justify-between gap-2 text-xs text-muted">
                      <span>{{ photo.format.toUpperCase() }}</span>
                      <span>{{ formatBytes(photo.byteSize) }}</span>
                    </p>
                  </div>
                </a>
              </div>

              <div v-else class="flex min-h-44 flex-col items-center justify-center text-center">
                <Icon name="tabler:photo-off" class="size-9 text-muted" />
                <p class="mt-3 font-medium">当前相簿还没有图片</p>
                <p class="mt-1 text-sm text-muted">从上方选择 PNG、JPG/JPEG 或 WEBP 图片上传。</p>
              </div>
            </UCard>
          </div>
        </div>
      </div>
    </template>
  </UDashboardPanel>
</template>
