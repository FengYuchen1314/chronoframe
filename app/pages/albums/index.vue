<script setup lang="ts">
import { motion } from 'motion-v'
import type { RustAlbum } from '~~/shared/types/photo'

const { t } = useI18n()
const { settings: siteSettings } = useSiteSettings()
const { photos } = usePhotos()
const { downloads } = usePublicAlbumDownloads()
const { data, status } = useFetch<RustAlbum[]>('/api/albums', {
  server: false,
  default: () => [],
})
const albums = computed(() => (data.value || []).map(album => adaptRustAlbum(album, photos.value)))
const isMobile = useMediaQuery('(max-width: 768px)')
const columnCount = computed(() => isMobile.value ? 3 : 8)
const rowCount = computed(() => isMobile.value ? 5 : 8)
const hoveredAlbum = ref<string | null>(null)

const backdropPhotos = computed(() => photos.value.slice(0, isMobile.value ? 16 : 40))
const columns = computed(() => {
  const result = Array.from({ length: columnCount.value }, () => [] as typeof photos.value)
  if (!backdropPhotos.value.length) return result
  for (let column = 0; column < result.length; column++) {
    for (let index = 0; index < rowCount.value; index++) {
      const photo = backdropPhotos.value[(column + index * result.length) % backdropPhotos.value.length]
      if (photo) result[column]?.push(photo)
    }
  }
  return result
})

const transform = (index: number, hover: boolean) => {
  if (index === 0) return { x: 0, y: 0, rotate: 0 }
  if (index === 1) return hover ? { x: -20, y: -16, rotate: -8 } : { x: -6, y: -4, rotate: -4 }
  return hover ? { x: 28, y: -20, rotate: 10 } : { x: 8, y: -6, rotate: 5 }
}

useHead({ title: t('title.albums') })
</script>

<template>
  <main class="relative min-h-svh overflow-hidden">
    <div class="absolute inset-x-0 top-0 -z-10 h-[30svh] overflow-hidden sm:h-[50vh]">
      <div class="absolute inset-0 flex">
        <div v-for="(column, columnIndex) in columns" :key="columnIndex" class="relative flex-1 overflow-hidden">
          <div class="flex flex-col" :class="columnIndex % 2 ? 'animate-scroll-up' : 'animate-scroll-down'" :style="{ animationDuration: `${72 + columnIndex * 7}s` }">
            <template v-for="copy in 3" :key="copy">
              <PhotoProgressiveImage
                v-for="photo in column"
                :key="`${copy}-${photo.id}`"
                :src="photo.thumbnailUrl"
                :alt="photo.title"
                loading="lazy"
                fetch-priority="low"
                fit="cover"
                class="w-full object-cover saturate-50"
                :style="{ aspectRatio: photo.aspectRatio || 1 }"
              />
            </template>
          </div>
        </div>
      </div>
      <div class="absolute -inset-1 bg-linear-to-b from-neutral-100/75 to-white dark:from-neutral-900/75 dark:to-neutral-900" />
    </div>

    <div class="albums-safe-top absolute left-3 z-10 sm:left-4">
      <UButton to="/photos" icon="tabler:arrow-left" :label="t('title.photos')" color="neutral" variant="ghost" size="md" class="min-h-11" />
    </div>

    <section class="flex flex-col items-center px-4 pb-10 pt-24 sm:pb-24 sm:pt-48">
      <h1 class="bg-linear-to-br from-neutral-800 to-neutral-400 bg-clip-text text-4xl font-black text-transparent drop-shadow-2xl sm:text-7xl dark:from-white dark:to-neutral-500">{{ t('title.albums').toUpperCase() }}</h1>
      <p v-if="siteSettings.slogan" class="mt-2 max-w-full truncate text-base font-medium font-[Pacifico] text-neutral-600 sm:text-lg dark:text-neutral-400">{{ siteSettings.slogan }}</p>
    </section>

    <section class="container mx-auto px-4 py-6 sm:px-6 sm:py-12 lg:px-8">
      <div v-if="status === 'idle' || status === 'pending'" class="grid min-h-64 place-items-center"><Icon name="tabler:loader-2" class="size-8 animate-spin text-primary" /></div>
      <div v-else-if="status === 'error'" class="grid min-h-64 place-items-center text-center text-neutral-500"><div><Icon name="tabler:cloud-off" class="mx-auto mb-3 size-10" /><p>{{ t('album.failedToLoad') }}</p></div></div>
      <div v-else-if="!albums.length" class="grid min-h-64 place-items-center text-center text-neutral-500"><div><Icon name="tabler:library-photo" class="mx-auto mb-3 size-12" /><p>{{ t('dashboard.albums.noAlbums') }}</p></div></div>

      <div v-else class="grid grid-cols-1 gap-10 sm:grid-cols-2 sm:gap-16 lg:grid-cols-3 xl:grid-cols-4">
        <article
          v-for="album in albums"
          :key="album.id"
          class="group relative"
          @mouseenter="hoveredAlbum = album.id"
          @mouseleave="hoveredAlbum = null"
        >
          <NuxtLink :to="`/albums/${album.id}`" class="block rounded-2xl outline-none transition-transform active:scale-[0.99] focus-visible:ring-2 focus-visible:ring-primary">
          <div class="relative mb-4 h-52 sm:h-48">
            <motion.div
              v-for="(photo, index) in album.photos.slice(0, 3)"
              :key="photo.id"
              class="absolute inset-0 overflow-hidden rounded-xl bg-white shadow-lg dark:bg-neutral-800"
              :initial="{ ...transform(index, false), opacity: 1 - index * 0.12 }"
              :animate="{ ...transform(index, hoveredAlbum === album.id), opacity: hoveredAlbum === album.id ? 1 : 1 - index * 0.12 }"
              :transition="{ type: 'spring', stiffness: 300, damping: 30, mass: 0.8 }"
              :style="{ zIndex: 3 - index }"
            >
              <PhotoProgressiveImage :src="photo.thumbnailUrl" :alt="album.title" class="h-full w-full" loading="lazy" fit="cover" />
              <motion.div v-if="index > 0" class="absolute inset-0 bg-black/15" :animate="{ opacity: hoveredAlbum === album.id ? 0 : 1 }" />
            </motion.div>

            <div v-if="!album.photos.length" class="absolute inset-0 flex flex-col items-center justify-center gap-3 rounded-xl border border-neutral-200 bg-linear-to-br from-neutral-100 to-neutral-50 shadow-lg transition-shadow group-hover:shadow-xl dark:border-neutral-600 dark:from-neutral-700 dark:to-neutral-800">
              <Icon name="tabler:library-photo" class="size-10 text-neutral-400" />
              <p class="text-sm font-medium text-neutral-600 dark:text-neutral-300">{{ t('ui.album.noImage') }}</p>
            </div>
          </div>

          <div class="px-2">
            <div class="flex items-center gap-4">
              <h2 class="min-w-0 flex-1 truncate text-lg font-semibold text-neutral-800 transition-colors group-hover:text-primary-600 dark:text-neutral-200 dark:group-hover:text-primary-400">{{ album.title }}</h2>
              <span class="flex shrink-0 items-center gap-1 text-sm text-neutral-500"><Icon name="tabler:photo" class="size-4" />{{ t('album.photo', album.photoCount) }}</span>
            </div>
            <p v-if="album.description" class="mt-1 line-clamp-2 text-sm leading-relaxed text-neutral-600 dark:text-neutral-400">{{ album.description }}</p>
            <p class="mt-1.5 text-sm text-neutral-500">{{ album.displayCreatedDate ? formatGalleryCalendarDate(album.displayCreatedDate) : formatGalleryDate(album.createdAt) }}</p>
          </div>
          </NuxtLink>
          <AlbumDownloadButton class="absolute right-2 top-2 z-10" :download="downloads.find(item => item.albumId === album.id)" />
        </article>
      </div>
    </section>
  </main>
</template>

<style scoped>
@keyframes scroll-down { from { transform: translateY(0); } to { transform: translateY(calc(-100% / 3)); } }
@keyframes scroll-up { from { transform: translateY(calc(-100% / 3)); } to { transform: translateY(0); } }
.animate-scroll-down { animation: scroll-down linear infinite; }
.animate-scroll-up { animation: scroll-up linear infinite; }
@media (prefers-reduced-motion: reduce) { .animate-scroll-down, .animate-scroll-up { animation: none; } }
.albums-safe-top { top: max(0.75rem, env(safe-area-inset-top)); }
</style>
