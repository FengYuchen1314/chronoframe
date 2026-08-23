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

const isLoadingAlbums = ref(false)
const isLoadingPhotos = ref(false)
const isCreating = ref(false)
const isUploading = ref(false)
const albumError = ref('')
const photoError = ref('')
let detailRequestSerial = 0

const selectedAlbum = computed(() =>
  albums.value.find(album => album.id === selectedAlbumId.value) || null,
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

const formatTime = (timestamp: number) =>
  new Date(timestamp * 1000).toLocaleString('zh-CN', { hour12: false })

const loadAlbumDetail = async (albumId: string) => {
  const requestSerial = ++detailRequestSerial
  photos.value = []
  photoError.value = ''

  if (!albumId) return
  isLoadingPhotos.value = true

  try {
    const detail = await adminFetch<AlbumDetail>(`/api/albums/${albumId}`)
    if (requestSerial !== detailRequestSerial || selectedAlbumId.value !== albumId) return

    photos.value = detail.photos
    const albumIndex = albums.value.findIndex(album => album.id === albumId)
    if (albumIndex >= 0) {
      albums.value[albumIndex] = {
        id: detail.id,
        name: detail.name,
        createdAt: detail.createdAt,
        photoCount: detail.photoCount,
      }
    }
  } catch (error) {
    if (requestSerial === detailRequestSerial) {
      photoError.value = getAdminApiErrorMessage(error)
    }
  } finally {
    if (requestSerial === detailRequestSerial) {
      isLoadingPhotos.value = false
    }
  }
}

const refreshAlbums = async (preferredAlbumId?: string) => {
  if (isLoadingAlbums.value) return
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
      await loadAlbumDetail(nextSelectedId)
    } else {
      selectedAlbumId.value = nextSelectedId
    }
  } catch (error) {
    albumError.value = getAdminApiErrorMessage(error)
  } finally {
    isLoadingAlbums.value = false
  }
}

const createAlbum = async () => {
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

const handleFileSelection = (event: Event) => {
  const input = event.target as HTMLInputElement
  selectedFiles.value = Array.from(input.files || [])
}

const clearSelectedFiles = () => {
  selectedFiles.value = []
  if (uploadInput.value) uploadInput.value.value = ''
}

const uploadPhotos = async () => {
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

  isUploading.value = true
  try {
    const uploaded = await adminFetch<Photo[]>(
      `/api/albums/${selectedAlbumId.value}/photos`,
      { method: 'POST', body: formData },
    )
    const albumId = selectedAlbumId.value
    clearSelectedFiles()
    await refreshAlbums(albumId)
    toast.add({
      title: '上传完成',
      description: `已将 ${uploaded.length} 张图片写入「${selectedAlbum.value?.name || '相簿'}」`,
      color: 'success',
    })
  } catch (error) {
    toast.add({
      title: '上传失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isUploading.value = false
  }
}

watch(selectedAlbumId, (albumId) => {
  clearSelectedFiles()
  void loadAlbumDetail(albumId)
})

onMounted(refreshAlbums)
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
                  />
                </UFormField>
                <UButton type="submit" block icon="tabler:plus" :loading="isCreating">
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
                  @click="selectedAlbumId = album.id"
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
                    <h2 class="text-lg font-semibold">{{ selectedAlbum.name }}</h2>
                    <p class="mt-1 text-sm text-muted">
                      创建于 {{ formatTime(selectedAlbum.createdAt) }} · {{ selectedAlbum.photoCount }} 张图片
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
                    :disabled="isUploading"
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
                    :disabled="isUploading"
                    @click="clearSelectedFiles"
                  >
                    清空选择
                  </UButton>
                </div>

                <div class="flex justify-end">
                  <UButton
                    icon="tabler:upload"
                    :loading="isUploading"
                    :disabled="!selectedFiles.length"
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
