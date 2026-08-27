<script setup lang="ts">
import type { GalleryPhoto } from '~~/shared/types/photo'
import { thumbnailWindow } from '~~/shared/utils/viewerPerformance'

const props = defineProps<{ photos: GalleryPhoto[]; currentIndex: number }>()
const emit = defineEmits<{ indexChange: [number] }>()
const container = ref<HTMLDivElement>()
const scrollLeft = ref(0)
const viewportWidth = ref(1280)
const size = 64
const gap = 12
const stride = size + gap
const padding = 16
const windowRange = computed(() => thumbnailWindow(scrollLeft.value, viewportWidth.value, props.photos.length, stride))
const visiblePhotos = computed(() => props.photos.slice(windowRange.value.start, windowRange.value.end)
  .map((photo, offset) => ({ photo, index: windowRange.value.start + offset })))
const contentWidth = computed(() => Math.max(0, props.photos.length * stride - gap))
const onScroll = () => { scrollLeft.value = container.value?.scrollLeft || 0 }
const scrollToActive = async (smooth = true) => {
  await nextTick()
  if (!container.value) return
  const left = props.currentIndex * stride
  if (left >= scrollLeft.value + padding && left + size <= scrollLeft.value + viewportWidth.value - padding) return
  container.value.scrollTo({ left: Math.max(0, left - container.value.clientWidth / 2 + size / 2), behavior: smooth ? 'smooth' : 'instant' })
  onScroll()
}
useResizeObserver(container, entries => {
  viewportWidth.value = entries[0]?.contentRect.width || 1280
  void scrollToActive(false)
})
watch(() => props.currentIndex, () => scrollToActive())
onMounted(() => scrollToActive(false))
const onWheel = (event: WheelEvent) => {
  if (container.value) container.value.scrollLeft += Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY
}
</script>

<template>
  <div class="z-10 shrink-0 border-t border-white/10 bg-black/70">
    <div ref="container" data-viewer-thumbnails class="scrollbar-none overflow-x-auto" :style="{ padding: `${padding}px` }" @scroll.passive="onScroll" @wheel.prevent="onWheel">
      <div class="relative" :style="{ width: `${contentWidth}px`, height: `${size}px` }">
        <button
          v-for="{ photo, index } in visiblePhotos"
          :key="photo.id"
          type="button"
          class="absolute top-0 overflow-hidden rounded-lg border-2 transition-transform duration-150"
          :class="index === currentIndex ? 'scale-110 border-white' : 'border-white/20 hover:border-white/60'"
          :style="{ left: `${index * stride}px`, width: `${size}px`, height: `${size}px` }"
          :aria-label="photo.title || $t('ui.photo.altFallback')"
          :aria-current="index === currentIndex ? 'true' : undefined"
          @click="emit('indexChange', index)"
        >
          <PhotoProgressiveImage :src="photo.thumbnailUrl" :alt="photo.title" class="absolute inset-0 h-full w-full" loading="lazy" fetch-priority="low" :pulse="false" fit="cover" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.scrollbar-none { scrollbar-width: none; }
.scrollbar-none::-webkit-scrollbar { display: none; }
</style>
