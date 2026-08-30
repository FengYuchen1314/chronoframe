<script lang="ts" setup>
import { Alert as AAlert, Button as AButton, Card as ACard, DatePicker as ADatePicker, Form as AForm, FormItem as AFormItem, Input as AInput, InputSearch as AInputSearch, Textarea as ATextarea, Modal as AModal, Table as ATable, Tabs as ATabs, TabPane as ATabPane, Space as ASpace, Tag as ATag, Progress as AProgress, UploadDragger as AUploadDragger, Image as AImage, Checkbox as ACheckbox } from 'ant-design-vue'
import type { Album, AlbumDeletionResult, AlbumDetail, Photo, PhotoDeletionResult } from '~/types/dashboard'

definePageMeta({
  layout: 'dashboard',
})

useHead({ title: '相簿' })

const toast = useAdminNotice()
const { adminFetch } = useAdminApi()

const UPLOAD_CONCURRENCY = 7

const albums = ref<Album[]>([])
const selectedAlbumId = ref('')
const photos = ref<Photo[]>([])
const activeWorkspaceTab = ref<'photos' | 'details' | 'export'>('photos')
const albumQuery = ref('')
const isCreateDialogOpen = ref(false)
const isDeleteDialogOpen = ref(false)
const isOrderMode = ref(false)
const newAlbumName = ref('')
const newAlbumDescription = ref('')
const selectedFiles = ref<File[]>([])
const selectedPhotoIds = ref<string[]>([])
const isSelectingPhotos = ref(false)
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
const albumNameDraft = ref('')
const savedAlbumNameDraft = ref('')
const albumDescriptionDraft = ref('')
const savedAlbumDescriptionDraft = ref('')
const descriptionDraftAlbumId = ref('')

const isLoadingAlbums = ref(false)
const isLoadingPhotos = ref(false)
const isCreating = ref(false)
const isUploading = ref(false)
const isDeletingPhotos = ref(false)
const isDeletingAlbum = ref(false)
const isSavingAlbumDates = ref(false)
const isSavingAlbumIdentity = ref(false)
const isReorderingAlbums = ref(false)
const isAlbumDetailReady = ref(false)
const uploadCompleted = ref(0)
const uploadTotal = ref(0)
const uploadActiveCount = ref(0)
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
const albumNameDirty = computed(() =>
  descriptionDraftAlbumId.value === selectedAlbumId.value
  && albumNameDraft.value !== savedAlbumNameDraft.value,
)
const albumIdentityDirty = computed(() => albumNameDirty.value || albumDescriptionDirty.value)
const albumMetadataDirty = computed(() => albumDatesDirty.value || albumIdentityDirty.value)
const albumHasCustomDates = computed(() => Boolean(
  selectedAlbum.value?.displayCreatedDate
  || selectedAlbum.value?.photoDateStart
  || selectedAlbum.value?.photoDateEnd,
))
const hasActiveMutation = computed(() =>
  isCreating.value
  || isUploading.value
  || isDeletingPhotos.value
  || isDeletingAlbum.value
  || isSavingAlbumDates.value
  || isSavingAlbumIdentity.value
  || isReorderingAlbums.value,
)
const isAlbumInteractionLocked = computed(() =>
  isLoadingAlbums.value || isLoadingPhotos.value || hasActiveMutation.value,
)

const selectedBytes = computed(() =>
  selectedFiles.value.reduce((total, file) => total + file.size, 0),
)
const allPhotosSelected = computed(() =>
  photos.value.length > 0 && selectedPhotoIds.value.length === photos.value.length,
)
const selectedExportAlbums = computed(() =>
  albums.value.filter(album => exportAlbumIds.value.includes(album.id)),
)

const workspaceTabs = [
  { value: 'photos' as const, label: '图片与上传', icon: 'tabler:photo' },
  { value: 'details' as const, label: '相簿设置', icon: 'tabler:settings' },
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

const applyAlbumIdentityDraft = (detail: AlbumDetail, force = false) => {
  if (!force && descriptionDraftAlbumId.value === detail.id && albumIdentityDirty.value) return
  descriptionDraftAlbumId.value = detail.id
  albumNameDraft.value = detail.name
  savedAlbumNameDraft.value = detail.name
  albumDescriptionDraft.value = detail.description || ''
  savedAlbumDescriptionDraft.value = detail.description || ''
}

const resetAlbumDateDraft = () => {
  albumDateDraft.displayCreatedDate = savedAlbumDateDraft.displayCreatedDate
  albumDateDraft.photoDateStart = savedAlbumDateDraft.photoDateStart
  albumDateDraft.photoDateEnd = savedAlbumDateDraft.photoDateEnd
}

const resetAlbumIdentityDraft = () => {
  albumNameDraft.value = savedAlbumNameDraft.value
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
  selectedPhotoIds.value = []
  photoError.value = ''
  isAlbumDetailReady.value = false

  if (!albumId) return true
  isLoadingPhotos.value = true

  try {
    const detail = await adminFetch<AlbumDetail>(`/api/albums/${albumId}`)
    if (requestSerial !== detailRequestSerial || selectedAlbumId.value !== albumId) return false

    photos.value = detail.photos
    applyAlbumDateDraft(detail)
    applyAlbumIdentityDraft(detail)
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
      : ''

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

const saveAlbumIdentity = async () => {
  if (isAlbumInteractionLocked.value || !isAlbumDetailReady.value) return
  if (!albumIdentityDirty.value || !selectedAlbumId.value) return
  const name = albumNameDraft.value.trim()
  const description = albumDescriptionDraft.value.trim()
  if (!name || Array.from(name).length > 100) {
    toast.add({ title: '相簿名不能为空且不能超过 100 个字符', color: 'warning' })
    return
  }
  if (description.length > 1000) {
    toast.add({ title: '相簿简介不能超过 1000 个字符', color: 'warning' })
    return
  }

  isSavingAlbumIdentity.value = true
  try {
    const updated = await adminFetch<Album>(`/api/albums/${selectedAlbumId.value}`, {
      method: 'PATCH',
      body: { name, description },
    })
    const albumIndex = albums.value.findIndex(album => album.id === updated.id)
    if (albumIndex >= 0) albums.value[albumIndex] = updated
    if (selectedAlbumId.value === updated.id) {
      applyAlbumIdentityDraft({ ...updated, photos: photos.value }, true)
    }
    toast.add({
      title: '相簿资料已保存',
      description: '名称和简介已同步到公开相簿。',
      color: 'success',
    })
  } catch (error) {
    toast.add({
      title: '保存相簿资料失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isSavingAlbumIdentity.value = false
  }
}

const requestDeleteCurrentAlbum = () => {
  const album = selectedAlbum.value
  if (!album || isAlbumInteractionLocked.value) return
  if (albumMetadataDirty.value) {
    toast.add({ title: '请先保存或放弃相簿资料修改', color: 'warning' })
    return
  }
  isDeleteDialogOpen.value = true
}

const closeDeleteAlbumDialog = () => {
  if (isDeletingAlbum.value) return
  isDeleteDialogOpen.value = false
}

const deleteCurrentAlbum = async () => {
  const album = selectedAlbum.value
  if (!album || isDeletingAlbum.value) return

  isDeletingAlbum.value = true
  isAlbumDetailReady.value = false
  try {
    const result = await adminFetch<AlbumDeletionResult>(`/api/albums/${album.id}`, {
      method: 'DELETE',
    })
    isDeleteDialogOpen.value = false
    selectedAlbumId.value = ''
    await refreshAlbums()
    toast.add({
      title: `已删除相簿「${album.name}」`,
      description: result.cleanupPending
        ? `${result.photosDeleted} 张图片已移出相簿；${result.cleanupPending} 个存储对象将在后台继续清理。`
        : `${result.photosDeleted} 张图片及其存储对象已清理。`,
      color: result.cleanupPending ? 'warning' : 'success',
    })
  } catch (error) {
    await refreshAlbums(album.id)
    toast.add({
      title: '删除相簿失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isDeletingAlbum.value = false
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

const togglePhotoSelection = (photoId: string) => {
  selectedPhotoIds.value = selectedPhotoIds.value.includes(photoId)
    ? selectedPhotoIds.value.filter(id => id !== photoId)
    : [...selectedPhotoIds.value, photoId]
}

const toggleAllPhotos = () => {
  selectedPhotoIds.value = allPhotosSelected.value ? [] : photos.value.map(photo => photo.id)
}

const leavePhotoSelection = () => {
  if (isDeletingPhotos.value) return
  isSelectingPhotos.value = false
  selectedPhotoIds.value = []
}

const deleteSelectedPhotos = async () => {
  if (isDeletingPhotos.value || !selectedPhotoIds.value.length) return
  const count = selectedPhotoIds.value.length
  if (!await toast.confirm(`确定永久删除选中的 ${count} 张图片吗？\n\n图片会从当前存储（包括 S3/R2 或 WebDAV）删除，不能撤销。`)) return
  const albumId = selectedAlbumId.value
  isDeletingPhotos.value = true
  isAlbumDetailReady.value = false
  try {
    const result = await adminFetch<PhotoDeletionResult>('/api/photos/delete', {
      method: 'POST',
      body: { photoIds: selectedPhotoIds.value },
    })
    selectedPhotoIds.value = []
    isSelectingPhotos.value = false
    const refreshed = await refreshAlbums(albumId)
    toast.add({
      title: `已删除 ${result.deleted} 张图片`,
      description: result.cleanupPending
        ? `${result.cleanupPending} 个存储对象暂未清理成功，后台会自动重试。`
        : '数据库记录和存储对象均已清理。',
      color: result.cleanupPending ? 'warning' : 'success',
    })
    if (!refreshed) photoError.value ||= '删除成功，但列表刷新失败，请手动刷新。'
  } catch (error) {
    toast.add({
      title: '删除图片失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
    await refreshAlbums(albumId)
  } finally {
    isDeletingPhotos.value = false
  }
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

  const albumId = selectedAlbumId.value
  const files = [...selectedFiles.value]
  const failed: Array<{ index: number, file: File, message: string }> = []
  let uploadedCount = 0
  let nextFileIndex = 0
  isUploading.value = true
  isAlbumDetailReady.value = false
  uploadCompleted.value = 0
  uploadTotal.value = files.length
  uploadActiveCount.value = 0
  try {
    const uploadWorker = async () => {
      while (nextFileIndex < files.length) {
        const index = nextFileIndex++
        const file = files[index]
        if (!file) continue

        uploadActiveCount.value += 1
        const formData = new FormData()
        formData.append('files', file, file.name)
        try {
          const uploaded = await adminFetch<Photo[]>(
            `/api/albums/${albumId}/photos`,
            { method: 'POST', body: formData },
          )
          uploadedCount += uploaded.length
          const knownIds = new Set(photos.value.map(photo => photo.id))
          const newPhotos = uploaded.filter(photo => !knownIds.has(photo.id))
          if (newPhotos.length) photos.value = [...newPhotos, ...photos.value]
        } catch (error) {
          failed.push({ index, file, message: getAdminApiErrorMessage(error) })
        } finally {
          uploadActiveCount.value -= 1
          uploadCompleted.value += 1
        }
      }
    }

    const workerCount = Math.min(UPLOAD_CONCURRENCY, files.length)
    await Promise.all(Array.from({ length: workerCount }, () => uploadWorker()))

    failed.sort((left, right) => left.index - right.index)
    selectedFiles.value = failed.map(item => item.file)
    if (!failed.length && uploadInput.value) uploadInput.value.value = ''
    const refreshed = await refreshAlbums(albumId)
    if (!refreshed) {
      toast.add({
        title: uploadedCount ? `已上传 ${uploadedCount} 张，列表同步失败` : '图片列表同步失败',
        description: albumError.value || photoError.value || '成功项已经保留，请点击刷新确认相簿状态。',
        color: 'warning',
      })
      return
    }
    if (failed.length) {
      const details = failed.slice(0, 3).map(item => `${item.file.name}：${item.message}`).join('；')
      toast.add({
        title: `已上传 ${uploadedCount} 张，${failed.length} 张失败`,
        description: `${details}${failed.length > 3 ? `；另有 ${failed.length - 3} 张失败` : ''}。失败文件已保留，可直接重试。`,
        color: uploadedCount ? 'warning' : 'error',
      })
    } else {
      toast.add({
        title: '上传完成',
        description: `已将 ${uploadedCount} 张图片写入「${selectedAlbum.value?.name || '相簿'}」`,
        color: 'success',
      })
    }
  } finally {
    isUploading.value = false
    uploadActiveCount.value = 0
    uploadCompleted.value = 0
    uploadTotal.value = 0
  }
}

watch(selectedAlbumId, (albumId) => {
  isDeleteDialogOpen.value = false
  clearSelectedFiles()
  selectedPhotoIds.value = []
  isSelectingPhotos.value = false
  isAlbumDetailReady.value = false
  dateDraftAlbumId.value = albumId
  albumDateDraft.displayCreatedDate = ''
  albumDateDraft.photoDateStart = ''
  albumDateDraft.photoDateEnd = ''
  savedAlbumDateDraft.displayCreatedDate = ''
  savedAlbumDateDraft.photoDateStart = ''
  savedAlbumDateDraft.photoDateEnd = ''
  descriptionDraftAlbumId.value = albumId
  albumNameDraft.value = ''
  savedAlbumNameDraft.value = ''
  albumDescriptionDraft.value = ''
  savedAlbumDescriptionDraft.value = ''
  void loadAlbumDetail(albumId)
})

const confirmDiscardAlbumMetadata = () =>
  !albumMetadataDirty.value || toast.confirm('相簿名称、简介或显示日期尚未保存，确定要放弃修改吗？')

const handleBeforeUnload = (event: BeforeUnloadEvent) => {
  if (!albumMetadataDirty.value && !isUploading.value) return
  event.preventDefault()
  event.returnValue = true
}

onBeforeRouteLeave(() => {
  if (isUploading.value) { toast.add({ title: '图片正在上传，请等待完成后离开', color: 'warning' }); return false }
  return confirmDiscardAlbumMetadata()
})
onMounted(() => {
  window.addEventListener('beforeunload', handleBeforeUnload)
  void refreshAlbums()
})
onBeforeUnmount(() => window.removeEventListener('beforeunload', handleBeforeUnload))
const albumColumns = [{ title: '排序', key: 'order', width: 105 }, { title: '相册名称', dataIndex: 'name' }, { title: '图片', dataIndex: 'photoCount', width: 90 }, { title: '简介', dataIndex: 'description', ellipsis: true }, { title: '操作', key: 'actions', width: 250 }]
const photoColumns = [{ title: '预览', key: 'preview', width: 90 }, { title: '文件名', dataIndex: 'originalName', ellipsis: true }, { title: '格式', dataIndex: 'format', width: 90 }, { title: '尺寸', key: 'dimensions', width: 130 }, { title: '大小', key: 'size', width: 110 }]
const photoSelection = computed(() => ({ selectedRowKeys: selectedPhotoIds.value, onChange: (keys: (string | number)[]) => { selectedPhotoIds.value = keys.map(String) }, getCheckboxProps: () => ({ disabled: isDeletingPhotos.value || isUploading.value }) }))
const queueFile = (file: File) => { selectedFiles.value.push(file); return false }
</script>

<template>
  <div>
    <DashboardPageHeader :title="selectedAlbum ? selectedAlbum.name : '相册管理'" :description="selectedAlbum ? '管理图片、相册资料和公开下载' : '创建相册、调整展示顺序并管理图片'">
      <AButton v-if="selectedAlbum" :disabled="isAlbumInteractionLocked" @click="selectAlbum('')">返回列表</AButton>
      <AButton :loading="isLoadingAlbums" :disabled="hasActiveMutation" @click="refreshAlbums()">刷新</AButton>
      <AButton v-if="!selectedAlbum" type="primary" @click="openCreateDialog">新建相册</AButton>
      <template v-else>
        <NuxtLink :to="'/dashboard/downloads?album=' + selectedAlbumId"><AButton>下载设置</AButton></NuxtLink>
        <NuxtLink :to="'/albums/' + selectedAlbumId" target="_blank"><AButton>查看相册</AButton></NuxtLink>
        <AButton danger :disabled="isAlbumInteractionLocked" @click="requestDeleteCurrentAlbum">删除相册</AButton>
      </template>
    </DashboardPageHeader>
    <AAlert v-if="albumError || photoError" class="mb-5" type="error" show-icon :message="albumError || photoError" />
    <ACard v-if="!selectedAlbum">
      <div class="admin-toolbar"><AInputSearch v-model:value="albumQuery" placeholder="搜索相册名称或简介" allow-clear style="width:320px;max-width:100%" /><span class="admin-help">共 {{ albums.length }} 个相册 · 上下箭头调整公开展示顺序</span></div>
      <ATable :columns="albumColumns" :data-source="filteredAlbums" row-key="id" :loading="isLoadingAlbums" :pagination="{ pageSize: 20, showSizeChanger: true }" :scroll="{ x: 900 }">
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'order'"><ASpace :size="0"><AButton type="text" :aria-label="'上移 ' + record.name" :disabled="albums[0]?.id === record.id || isAlbumInteractionLocked" @click="moveAlbum(record.id, -1)"><Icon name="tabler:arrow-up" /></AButton><AButton type="text" :aria-label="'下移 ' + record.name" :disabled="albums.at(-1)?.id === record.id || isAlbumInteractionLocked" @click="moveAlbum(record.id, 1)"><Icon name="tabler:arrow-down" /></AButton></ASpace></template>
          <template v-else-if="column.dataIndex === 'name'"><AButton type="link" style="padding:0" @click="selectAlbum(record.id); activeWorkspaceTab = 'photos'">{{ record.name }}</AButton></template>
          <template v-else-if="column.key === 'actions'"><ASpace><AButton type="link" size="small" @click="selectAlbum(record.id); activeWorkspaceTab = 'photos'">图片</AButton><AButton type="link" size="small" @click="selectAlbum(record.id); activeWorkspaceTab = 'details'">编辑</AButton><NuxtLink :to="'/dashboard/downloads?album=' + record.id">下载设置</NuxtLink></ASpace></template>
        </template>
      </ATable>
      <div class="mt-4"><AButton @click="activeWorkspaceTab = 'export'">管理员批量导出</AButton></div>
    </ACard>

    <ATabs v-if="selectedAlbum" v-model:active-key="activeWorkspaceTab" class="mt-2">
      <ATabPane key="photos" tab="图片与上传">
        <div class="admin-stack">
          <ACard title="上传图片">
            <AUploadDragger multiple accept=".png,.jpg,.jpeg,.webp" :file-list="[]" :show-upload-list="false" :before-upload="queueFile" :disabled="isUploading || !isAlbumDetailReady">
              <p class="ant-upload-drag-icon"><Icon name="tabler:cloud-upload" style="font-size:36px;color:#1677ff" /></p>
              <p class="ant-upload-text">点击或拖动图片到这里</p>
              <p class="ant-upload-hint">支持 PNG、JPG / JPEG、WebP，默认 7 并发上传</p>
            </AUploadDragger>
            <div v-if="selectedFiles.length || isUploading" class="mt-4">
              <div class="admin-toolbar"><span>已选择 {{ selectedFiles.length }} 张 · {{ formatBytes(selectedBytes) }}</span><ASpace><AButton :disabled="isUploading" @click="clearSelectedFiles">清空</AButton><AButton type="primary" :loading="isUploading" :disabled="!selectedFiles.length" @click="uploadPhotos">开始上传</AButton></ASpace></div>
              <AProgress v-if="isUploading" :percent="uploadTotal ? Math.round(uploadCompleted / uploadTotal * 100) : 0" />
              <p v-if="isUploading" class="admin-help">{{ uploadCompleted }} / {{ uploadTotal }} · {{ uploadActiveCount }} 个文件正在上传，请保持当前页面打开</p>
              <p v-else class="admin-help" style="overflow-wrap:anywhere">{{ selectedFiles.slice(0, 5).map(file => file.name).join('、') }}{{ selectedFiles.length > 5 ? ' 等' : '' }}</p>
            </div>
          </ACard>
          <ACard :title="'相册图片（' + photos.length + '）'">
            <template #extra><AButton danger :disabled="!selectedPhotoIds.length || isUploading" :loading="isDeletingPhotos" @click="deleteSelectedPhotos">删除所选 {{ selectedPhotoIds.length || '' }}</AButton></template>
            <ATable :columns="photoColumns" :data-source="photos" :row-selection="photoSelection" row-key="id" :loading="isLoadingPhotos" :pagination="{ pageSize: 30, showSizeChanger: true }" :scroll="{ x: 620 }">
              <template #bodyCell="{ column, record }">
                <template v-if="column.key === 'preview'"><AImage :width="48" :height="48" class="admin-preview" :src="'/api/photos/' + record.id + '/thumbnail'" :preview="{ src: '/api/photos/' + record.id + '/preview' }" /></template>
                <template v-else-if="column.dataIndex === 'format'"><ATag>{{ record.format.toUpperCase() }}</ATag></template>
                <template v-else-if="column.key === 'dimensions'">{{ record.width }} × {{ record.height }}</template>
                <template v-else-if="column.key === 'size'">{{ formatBytes(record.byteSize) }}</template>
              </template>
            </ATable>
          </ACard>
        </div>
      </ATabPane>
      <ATabPane key="details" tab="相册设置">
        <div class="admin-stack" style="max-width:960px">
          <ACard title="基本信息"><AForm layout="vertical" @finish="saveAlbumIdentity">
            <AFormItem label="相册名称" html-for="album-name" required><AInput id="album-name" v-model:value="albumNameDraft" :maxlength="100" :disabled="!isAlbumDetailReady" /></AFormItem>
            <AFormItem label="相册简介" html-for="album-description" extra="显示在相册首页和详情页"><ATextarea id="album-description" v-model:value="albumDescriptionDraft" :maxlength="1000" show-count :rows="4" :disabled="!isAlbumDetailReady" /></AFormItem>
            <ASpace><AButton type="primary" html-type="submit" :loading="isSavingAlbumIdentity" :disabled="!albumIdentityDirty">保存基本信息</AButton><AButton :disabled="!albumIdentityDirty" @click="resetAlbumIdentityDraft">重置</AButton></ASpace>
          </AForm></ACard>
          <ACard title="展示日期"><AForm layout="vertical" @finish="saveAlbumDates">
            <div class="admin-form-grid">
              <AFormItem label="相册创建日期" html-for="album-created-date"><ADatePicker id="album-created-date" v-model:value="albumDateDraft.displayCreatedDate" value-format="YYYY-MM-DD" :allow-clear="false" /></AFormItem><div />
              <AFormItem label="图片开始日期" html-for="album-start-date"><ADatePicker id="album-start-date" v-model:value="albumDateDraft.photoDateStart" value-format="YYYY-MM-DD" :allow-clear="false" /></AFormItem>
              <AFormItem label="图片结束日期" html-for="album-end-date"><ADatePicker id="album-end-date" v-model:value="albumDateDraft.photoDateEnd" value-format="YYYY-MM-DD" :allow-clear="false" /></AFormItem>
            </div>
            <ASpace wrap><AButton type="primary" html-type="submit" :loading="isSavingAlbumDates" :disabled="!albumDatesDirty">保存日期</AButton><AButton :disabled="!albumDatesDirty" @click="resetAlbumDateDraft">重置</AButton><AButton @click="clearAlbumDates">恢复自动日期</AButton></ASpace>
          </AForm></ACard>
        </div>
      </ATabPane>
      <ATabPane key="export" tab="管理员导出">
        <AAlert type="info" show-icon message="访客下载请在“下载设置”中开启。此处仅供管理员导出原始文件。" class="mb-4" />
      </ATabPane>
    </ATabs>
    <ACard v-if="activeWorkspaceTab === 'export'" title="管理员批量导出" class="mt-4">
      <p class="admin-help mb-4">单个相册为一个 ZIP；多个相册为一个包含各相册 ZIP 的总包。此导出不改变公开下载设置。</p>
      <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 mb-5"><ACheckbox v-for="album in albums" :key="album.id" :checked="exportAlbumIds.includes(album.id)" @change="toggleExportAlbum(album.id, $event.target.checked)">{{ album.name }}（{{ album.photoCount }}）</ACheckbox></div>
      <ASpace><AButton type="primary" :loading="isStartingExport" :disabled="!exportAlbumIds.length" @click="startAlbumExport">打包并下载所选相册</AButton><AButton @click="activeWorkspaceTab = 'photos'">收起</AButton></ASpace>
    </ACard>
    <AModal :open="isCreateDialogOpen" title="新建相册" ok-text="创建相册" cancel-text="取消" :confirm-loading="isCreating" :mask-closable="!isCreating" @ok="createAlbum" @cancel="closeCreateDialog">
      <AForm layout="vertical" class="mt-5"><AFormItem label="相册名称" html-for="new-album-name" required><AInput id="new-album-name" v-model:value="newAlbumName" :maxlength="100" @press-enter="createAlbum" /></AFormItem><AFormItem label="相册简介" html-for="new-album-description"><ATextarea id="new-album-description" v-model:value="newAlbumDescription" :rows="3" :maxlength="1000" /></AFormItem></AForm>
    </AModal>
    <AModal :open="isDeleteDialogOpen" title="删除相册" ok-text="永久删除" cancel-text="取消" :ok-button-props="{ danger: true }" :confirm-loading="isDeletingAlbum" :mask-closable="!isDeletingAlbum" @ok="deleteCurrentAlbum" @cancel="closeDeleteAlbumDialog">
      <AAlert type="warning" show-icon :message="'确定删除「' + (selectedAlbum?.name || '') + '」？'" :description="'该相册的 ' + (selectedAlbum?.photoCount || 0) + ' 张图片、存储原文件及本地下载包都会删除，此操作不能撤销。'" />
    </AModal>
  </div>
</template>
