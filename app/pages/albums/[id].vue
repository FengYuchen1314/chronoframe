<script setup lang="ts">
import { motion } from 'motion-v'
import type { RustAlbumDetailPayload } from '~~/shared/types/photo'

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const { photos: globalPhotos } = usePhotos()
const albumId = computed(() => String(route.params.id || ''))
const { data, status, error } = useAsyncData(
  'album-detail',
  () => $fetch<RustAlbumDetailPayload>(`/api/albums/${encodeURIComponent(albumId.value)}`),
  { server: false, watch: [albumId] },
)
watch(albumId, () => { data.value = undefined }, { flush: 'sync' })
const album = computed(() => data.value ? adaptRustAlbumDetail(data.value, globalPhotos.value) : null)
const viewer = useViewerState()
const isMobile = useMediaQuery('(max-width: 768px)')
const showTop = ref(false)
const items = computed(() => (album.value?.photos || []).map((photo, index) => ({ id: photo.id, photo, index })))
const keyMapper = (item: { id: string }) => item.id
const cover = computed(() => album.value?.photos.find(photo => photo.id === album.value?.coverPhotoId) || album.value?.photos[0])
const dateRange = computed(() => {
  const dates = (album.value?.photos || []).map(photo => new Date(photo.dateTaken)).filter(date => date.getTime() > 0).sort((a, b) => a.getTime() - b.getTime())
  if (!dates.length) return ''
  return dates.length === 1 ? formatGalleryDate(dates[0]!.toISOString()) : `${formatGalleryDate(dates[0]!.toISOString())} – ${formatGalleryDate(dates.at(-1)!.toISOString())}`
})

const openPhoto = (index: number) => {
  const photo = album.value?.photos[index]
  if (!photo || !album.value) return
  viewer.openViewer(index, `/albums/${album.value.id}`, album.value.photos)
  router.push(`/${photo.id}`)
}
const onScroll = () => { showTop.value = window.scrollY > 500 }
const scrollToTop = () => window.scrollTo({ top: 0, behavior: 'smooth' })
onMounted(() => window.addEventListener('scroll', onScroll, { passive: true }))
onBeforeUnmount(() => window.removeEventListener('scroll', onScroll))
useHead({ title: computed(() => album.value?.title || t('title.albums')) })
</script>

<template>
  <main class="relative min-h-svh">
    <div v-if="status === 'idle' || status === 'pending'" class="grid min-h-[60vh] place-items-center"><Icon name="tabler:loader-2" class="size-8 animate-spin text-primary" /></div>

    <template v-else-if="album">
      <div v-if="cover" class="absolute inset-x-0 top-0 -z-10 h-[500px] overflow-hidden">
        <img :src="cover.thumbnailUrl" :alt="album.title" class="h-full w-full scale-110 object-cover opacity-40 saturate-150 dark:opacity-20" />
        <div class="absolute -inset-1 bg-linear-to-b from-transparent via-white/50 to-white backdrop-blur-xl sm:backdrop-blur-2xl dark:via-neutral-900/50 dark:to-neutral-900" />
      </div>

      <div class="container mx-auto px-4 pt-4 sm:px-6 lg:px-8">
        <UButton to="/albums" icon="tabler:arrow-left" color="neutral" variant="ghost" size="sm" />
      </div>

      <section class="container mx-auto px-4 py-8 sm:px-6 lg:px-8">
        <motion.div :initial="{ opacity: 0, y: 10 }" :animate="{ opacity: 1, y: 0 }" :transition="{ duration: 0.4 }" class="flex flex-col gap-5">
          <h1 class="text-3xl font-bold tracking-tight text-neutral-900 sm:text-4xl dark:text-white">{{ album.title }}</h1>
          <div class="flex flex-wrap items-center gap-4 text-sm text-neutral-600 dark:text-neutral-300">
            <span class="flex items-center gap-1"><Icon name="tabler:photo" class="size-4 text-neutral-400" />{{ t('album.photo', album.photoCount) }}</span>
            <span v-if="dateRange" class="flex items-center gap-1"><Icon name="tabler:calendar" class="size-4 text-neutral-400" />{{ dateRange }}</span>
            <span class="flex items-center gap-1"><Icon name="tabler:clock-plus" class="size-4 text-neutral-400" />{{ formatGalleryDate(album.createdAt) }}</span>
          </div>
        </motion.div>
      </section>

      <section class="container mx-auto px-1 py-8 sm:px-4 lg:px-8">
        <div v-if="!items.length" class="grid min-h-64 place-items-center text-center text-neutral-500"><div><Icon name="tabler:library-photo" class="mx-auto mb-3 size-14" /><p>{{ t('album.emptyAlbumTitle') }}</p></div></div>
        <MasonryWall
          v-else
          :items="items"
          :column-width="280"
          :gap="4"
          :min-columns="2"
          :max-columns="isMobile ? 2 : 8"
          :ssr-columns="2"
          :key-mapper="keyMapper"
        >
          <template #default="{ item }">
            <MasonryItem :photo="item.photo" :index="item.index" @open-viewer="openPhoto" />
          </template>
        </MasonryWall>
      </section>
    </template>

    <div v-else-if="error" class="grid min-h-[60vh] place-items-center text-center">
      <div><Icon name="tabler:alert-circle" class="mx-auto mb-4 size-14 text-red-400" /><h1 class="text-2xl font-semibold">{{ t('album.failedToLoadTitle') }}</h1><UButton to="/albums" class="mt-6" :label="t('album.backToAlbums')" /></div>
    </div>

    <motion.div v-if="showTop" class="fixed bottom-6 right-6 z-40" :initial="{ opacity: 0, scale: 0.8 }" :animate="{ opacity: 1, scale: 1 }">
      <UButton icon="tabler:arrow-up" color="neutral" variant="soft" size="lg" class="rounded-full bg-white/80 shadow-lg backdrop-blur dark:bg-neutral-900/80" @click="scrollToTop" />
    </motion.div>
  </main>
</template>
