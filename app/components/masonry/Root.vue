<script setup lang="ts">
import { motion } from 'motion-v'
import type { AsyncDataRequestStatus } from '#app'
import type { GalleryPhoto } from '~~/shared/types/photo'

defineProps<{ photos: GalleryPhoto[]; status: AsyncDataRequestStatus }>()
const router = useRouter()
const isMobile = useMediaQuery('(max-width: 768px)')
const { filteredPhotos, hasActiveFilters } = usePhotoFilters()
const { sortedPhotos } = usePhotoSort()
const viewer = useViewerState()
const showTop = ref(false)
const masonryWrapper = ref<HTMLElement>()
const headerRef = ref<HTMLElement>()
const headerHeight = ref(0)
const headerColumnWidth = ref(280)
const gap = 4

const displayPhotos = computed(() => hasActiveFilters.value ? filteredPhotos.value : sortedPhotos.value)
const items = computed(() => displayPhotos.value.map((photo, index) => ({ photo, index, id: photo.id })))
const keyMapper = (item: { id: string }) => item.id
const columnWidth = computed(() => 280)
const maxColumns = computed(() => isMobile.value ? 2 : 8)
const headerOffset = computed(() => isMobile.value ? 0 : headerHeight.value + gap)
const headerStyle = computed(() => isMobile.value
  ? { width: '100%', marginBottom: `${gap}px` }
  : { width: `${headerColumnWidth.value}px` })

const updateHeaderWidth = () => {
  if (isMobile.value) return
  const column = masonryWrapper.value?.querySelector<HTMLElement>('.masonry-wall .masonry-column')
  headerColumnWidth.value = column?.getBoundingClientRect().width || columnWidth.value
}

useResizeObserver(headerRef, (entries) => {
  const entry = entries[0]
  if (entry) headerHeight.value = entry.contentRect.height
})
useResizeObserver(masonryWrapper, updateHeaderWidth)
watch(isMobile, () => nextTick(updateHeaderWidth))
watch(() => items.value.length, () => nextTick(updateHeaderWidth))

const dateRangeText = computed(() => {
  const dates = displayPhotos.value
    .map(photo => new Date(photo.dateTaken))
    .filter(date => !Number.isNaN(date.getTime()) && date.getTime() > 0)
    .sort((a, b) => b.getTime() - a.getTime())
  if (!dates.length) return ''
  return `${formatGalleryDate(dates.at(-1)!.toISOString())} – ${formatGalleryDate(dates[0]!.toISOString())}`
})

const openPhoto = (index: number) => {
  const photo = displayPhotos.value[index]
  if (!photo) return
  viewer.openViewer(index, '/photos', displayPhotos.value)
  router.push(`/${photo.id}`)
}

const onScroll = () => { showTop.value = window.scrollY > 500 }
const scrollToTop = () => window.scrollTo({ top: 0, behavior: 'smooth' })
onMounted(() => window.addEventListener('scroll', onScroll, { passive: true }))
onMounted(() => nextTick(updateHeaderWidth))
onBeforeUnmount(() => window.removeEventListener('scroll', onScroll))
</script>

<template>
  <div class="relative w-full">
    <div class="p-1" :class="isMobile && 'pt-2'">
      <div
        ref="masonryWrapper"
        class="relative"
        :style="{ '--masonry-header-offset': `${headerOffset}px` }"
      >
        <div
          ref="headerRef"
          class="masonry-header-wrapper"
          :class="!isMobile && 'masonry-header-desktop'"
          :style="headerStyle"
        >
          <MasonryItemHeader :total="displayPhotos.length" :date-range-text="dateRangeText" />
        </div>

        <div v-if="(status === 'idle' || status === 'pending') && !photos.length" class="grid min-h-[45vh] place-items-center" :style="!isMobile ? { paddingTop: `${headerOffset}px` } : undefined">
          <Icon name="tabler:loader-2" class="size-8 animate-spin text-primary" />
        </div>
        <div v-else-if="status === 'error' && !photos.length" class="grid min-h-[45vh] place-items-center text-center text-neutral-500" :style="!isMobile ? { paddingTop: `${headerOffset}px` } : undefined">
          <div><Icon name="tabler:cloud-off" class="mx-auto mb-3 size-10" /><p>{{ $t('ui.photo.loadError') }}</p></div>
        </div>
        <div v-else-if="!displayPhotos.length" class="grid min-h-[35vh] place-items-center text-center text-neutral-500" :style="!isMobile ? { paddingTop: `${headerOffset}px` } : undefined">
          <div><Icon name="tabler:photo-off" class="mx-auto mb-3 size-10" /><p>{{ $t('ui.stats.noPhotosTip') }}</p></div>
        </div>
        <MasonryWall
          v-else
          class="masonry-wall masonry-wall-with-header"
          :items="items"
          :column-width="columnWidth"
          :gap="gap"
          :min-columns="2"
          :max-columns="maxColumns"
          :ssr-columns="2"
          :key-mapper="keyMapper"
        >
          <template #default="{ item }">
            <MasonryItem :photo="item.photo" :index="item.index" @open-viewer="openPhoto" />
          </template>
        </MasonryWall>
      </div>
    </div>

    <motion.div
      v-if="showTop"
      class="fixed bottom-6 right-6 z-40"
      :initial="{ opacity: 0, scale: 0.8 }"
      :animate="{ opacity: 1, scale: 1 }"
    >
      <UButton icon="tabler:arrow-up" color="neutral" variant="soft" size="lg" class="rounded-full bg-white/80 shadow-lg backdrop-blur dark:bg-neutral-900/80" @click="scrollToTop" />
    </motion.div>
  </div>
</template>

<style scoped>
.masonry-header-wrapper { z-index: 1; }
.masonry-header-desktop { left: 0; position: absolute; top: 0; }
.masonry-wall-with-header :deep(.masonry-column:first-child) { padding-top: var(--masonry-header-offset, 0px); }
.masonry-wall-with-header :deep(.masonry-column:first-child .masonry-item:first-child) { margin-top: 0; }
</style>
