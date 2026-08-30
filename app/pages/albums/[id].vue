<script setup lang="ts">
import { motion } from 'motion-v'
import type { GalleryPhoto, RustAlbumDetailPayload } from '~~/shared/types/photo'

definePageMeta({ key: route => route.path })
const route = useRoute()
const { t } = useI18n()
const { downloads } = usePublicAlbumDownloads()
const albumId = computed(() => String(route.params.id || ''))
const { data, status, error } = useAsyncData(
  'album-detail',
  () => $fetch<RustAlbumDetailPayload>(`/api/albums/${encodeURIComponent(albumId.value)}`),
  { server: false, watch: [albumId] },
)
watch(albumId, () => { data.value = undefined }, { flush: 'sync' })
const album = computed(() => data.value ? adaptRustAlbumDetail(data.value) : null)
const isMobile = useMediaQuery('(max-width: 768px)')
const showTop = ref(false)
const albumPhotos = computed(() => album.value?.photos || [])
const { openPhoto } = useViewerRoute(albumPhotos, () => `/albums/${albumId.value}`)
const items = computed(() => albumPhotos.value.map((photo, index) => ({ id: photo.id, photo, index })))
const dateRange = computed(() => {
  if (album.value?.photoDateStart && album.value.photoDateEnd) {
    const start = formatGalleryCalendarDate(album.value.photoDateStart)
    const end = formatGalleryCalendarDate(album.value.photoDateEnd)
    return start === end ? start : `${start} – ${end}`
  }
  const dates = (album.value?.photos || []).map(photo => new Date(photo.dateTaken)).filter(date => date.getTime() > 0).sort((a, b) => a.getTime() - b.getTime())
  if (!dates.length) return ''
  const start = formatGalleryDate(dates[0]!.toISOString())
  const end = formatGalleryDate(dates.at(-1)!.toISOString())
  return start === end ? start : `${start} – ${end}`
})
const createdDate = computed(() => {
  if (!album.value) return ''
  return album.value.displayCreatedDate
    ? formatGalleryCalendarDate(album.value.displayCreatedDate)
    : formatGalleryDate(album.value.createdAt)
})
const selectionMode = ref(false)
const selectedIds = ref(new Set<string>())
const selectionAnchor = ref<number | null>(null)
const actionMenu = reactive<{ open: boolean; x: number; y: number; photo: GalleryPhoto | null }>({ open: false, x: 0, y: 0, photo: null })
const selectedPhotos = computed(() => albumPhotos.value.filter(photo => selectedIds.value.has(photo.id)))
const replaceSelection = (ids: Iterable<string>) => { selectedIds.value = new Set(ids) }
const cancelSelection = () => {
  selectionMode.value = false
  selectedIds.value = new Set()
  selectionAnchor.value = null
}
const toggleSelection = (index: number, event?: MouseEvent | KeyboardEvent) => {
  const photo = albumPhotos.value[index]
  if (!photo) return
  selectionMode.value = true
  const next = new Set(selectedIds.value)
  if (event?.shiftKey && selectionAnchor.value !== null && !isMobile.value) {
    const start = Math.min(selectionAnchor.value, index)
    const end = Math.max(selectionAnchor.value, index)
    for (let current = start; current <= end; current += 1) {
      const item = albumPhotos.value[current]
      if (item) next.add(item.id)
    }
  } else if (next.has(photo.id)) next.delete(photo.id)
  else next.add(photo.id)
  selectionAnchor.value = index
  replaceSelection(next)
  if (!next.size) cancelSelection()
}
const openActionMenu = (photo: GalleryPhoto | null, x: number, y: number) => Object.assign(actionMenu, { open: true, photo, x, y })
const enterSelection = (photo: GalleryPhoto | null) => {
  selectionMode.value = true
  if (!photo) return
  const index = albumPhotos.value.findIndex(item => item.id === photo.id)
  if (index >= 0 && !selectedIds.value.has(photo.id)) toggleSelection(index)
}
const selectAll = () => {
  selectionMode.value = true
  replaceSelection(albumPhotos.value.map(photo => photo.id))
}

const handleActivate = (index: number, event: MouseEvent | KeyboardEvent) => {
  if (selectionMode.value) toggleSelection(index, event)
  else openPhoto(index)
}
const onEmptyContextMenu = (event: MouseEvent) => {
  if ((event.target as Element).closest('[data-photo-id]')) return
  event.preventDefault()
  openActionMenu(null, event.clientX, event.clientY)
}
const onScroll = () => { showTop.value = window.scrollY > 500 }
const scrollToTop = () => window.scrollTo({ top: 0, behavior: 'smooth' })
onMounted(() => window.addEventListener('scroll', onScroll, { passive: true }))
onBeforeUnmount(() => window.removeEventListener('scroll', onScroll))
useEventListener('keydown', (event: KeyboardEvent) => {
  if (event.key === 'Escape' && selectionMode.value) cancelSelection()
})
watch(() => albumPhotos.value.map(photo => photo.id).join('\u0000'), () => {
  const available = new Set(albumPhotos.value.map(photo => photo.id))
  replaceSelection([...selectedIds.value].filter(id => available.has(id)))
  if (!selectedIds.value.size) selectionMode.value = false
})
useHead({ title: computed(() => album.value?.title || t('title.albums')) })
</script>

<template>
  <main class="relative min-h-svh">
    <div v-if="status === 'idle' || status === 'pending'" class="grid min-h-[60vh] place-items-center"><Icon name="tabler:loader-2" class="size-8 animate-spin text-primary" /></div>

    <template v-else-if="album">
      <div v-if="album.coverUrl" class="absolute inset-x-0 top-0 -z-10 h-[320px] overflow-hidden sm:h-[500px]">
        <PhotoProgressiveImage :src="album.coverUrl" :alt="album.title" loading="eager" fetch-priority="high" fit="cover" class="h-full w-full scale-110 opacity-40 saturate-150 dark:opacity-20" />
        <div class="absolute -inset-1 bg-linear-to-b from-transparent via-white/50 to-white backdrop-blur-xl sm:backdrop-blur-2xl dark:via-neutral-900/50 dark:to-neutral-900" />
      </div>

      <div class="album-detail-safe-top container mx-auto px-3 sm:px-6 lg:px-8">
        <UButton to="/albums" icon="tabler:arrow-left" color="neutral" variant="ghost" size="md" class="min-h-11 min-w-11" />
      </div>

      <section class="container mx-auto px-4 pb-5 pt-6 sm:px-6 sm:py-8 lg:px-8">
        <motion.div :initial="{ opacity: 0, y: 10 }" :animate="{ opacity: 1, y: 0 }" :transition="{ duration: 0.4 }" class="flex flex-col gap-5">
          <div class="flex flex-wrap items-center justify-between gap-4">
            <h1 class="text-2xl font-bold tracking-tight text-neutral-900 sm:text-4xl dark:text-white">{{ album.title }}</h1>
        <AlbumDownloadButton :download="downloads.find(item => item.albumId === albumId)" />
          </div>
          <p v-if="album.description" class="max-w-3xl whitespace-pre-line text-base leading-relaxed text-neutral-600 dark:text-neutral-300">{{ album.description }}</p>
          <div class="flex flex-wrap items-center gap-x-4 gap-y-2 text-sm text-neutral-600 dark:text-neutral-300">
            <span class="flex items-center gap-1"><Icon name="tabler:photo" class="size-4 text-neutral-400" />{{ t('album.photo', album.photoCount) }}</span>
            <span v-if="dateRange" class="flex items-center gap-1"><Icon name="tabler:calendar" class="size-4 text-neutral-400" />{{ dateRange }}</span>
            <span class="flex items-center gap-1"><Icon name="tabler:clock-plus" class="size-4 text-neutral-400" />{{ createdDate }}</span>
          </div>
        </motion.div>
      </section>

      <section class="container mx-auto px-0.5 pb-10 pt-5 sm:px-4 sm:py-8 lg:px-8" @contextmenu="onEmptyContextMenu">
        <div v-if="!items.length" class="grid min-h-64 place-items-center text-center text-neutral-500"><div><Icon name="tabler:library-photo" class="mx-auto mb-3 size-14" /><p>{{ t('album.emptyAlbumTitle') }}</p></div></div>
        <MasonryGallery
          v-else
          :items="items"
          :column-width="280"
          :gap="4"
          :min-columns="2"
          :max-columns="isMobile ? 2 : 8"
        >
          <template #default="{ item }">
            <MasonryItem
              :photo="item.photo"
              :index="item.index"
              :selected="selectedIds.has(item.photo.id)"
              :selection-mode="selectionMode"
              @activate="handleActivate"
              @select="toggleSelection"
              @context-action="openActionMenu"
            />
          </template>
        </MasonryGallery>
      </section>
    </template>

    <div v-else-if="error" class="grid min-h-[60vh] place-items-center text-center">
      <div><Icon name="tabler:alert-circle" class="mx-auto mb-4 size-14 text-red-400" /><h1 class="text-2xl font-semibold">{{ t('album.failedToLoadTitle') }}</h1><UButton to="/albums" class="mt-6" :label="t('album.backToAlbums')" /></div>
    </div>

    <motion.div v-if="showTop" class="album-back-to-top fixed right-4 z-40 sm:right-6" :initial="{ opacity: 0, scale: 0.8 }" :animate="{ opacity: 1, scale: 1 }">
      <UButton icon="tabler:arrow-up" color="neutral" variant="soft" size="lg" class="rounded-full bg-white/80 shadow-lg backdrop-blur dark:bg-neutral-900/80" @click="scrollToTop" />
    </motion.div>
    <PhotoActionMenu :open="actionMenu.open" :x="actionMenu.x" :y="actionMenu.y" :photo="actionMenu.photo" allow-select @close="actionMenu.open = false" @select="enterSelection" />
    <PhotoBulkActionBar :selected="selectedPhotos" :all-count="albumPhotos.length" @cancel="cancelSelection" @select-all="selectAll" />
  </main>
</template>

<style scoped>
.album-detail-safe-top { padding-top: max(0.75rem, env(safe-area-inset-top)); }
.album-back-to-top { bottom: max(1rem, env(safe-area-inset-bottom)); }
</style>
