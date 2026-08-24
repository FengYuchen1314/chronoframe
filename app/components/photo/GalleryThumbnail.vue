<script setup lang="ts">
import { motion } from 'motion-v'
import type { GalleryPhoto } from '~~/shared/types/photo'

const props = defineProps<{ photos: GalleryPhoto[]; currentIndex: number }>()
const emit = defineEmits<{ indexChange: [number] }>()
const container = ref<HTMLDivElement>()
const isMobile = useMediaQuery('(max-width: 768px)')
const size = computed(() => isMobile.value ? 48 : 64)
const gap = computed(() => isMobile.value ? 8 : 12)
const padding = computed(() => isMobile.value ? 12 : 16)

const scrollToActive = async () => {
  await nextTick()
  if (!container.value) return
  const left = props.currentIndex * (size.value + gap.value)
  container.value.scrollTo({ left: left - container.value.clientWidth / 2 + size.value / 2, behavior: 'smooth' })
}
const onWheel = (event: WheelEvent) => {
  if (!container.value) return
  event.preventDefault()
  container.value.scrollLeft += Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY
}

watch(() => props.currentIndex, scrollToActive, { immediate: true })
watch(isMobile, scrollToActive)
onMounted(() => container.value?.addEventListener('wheel', onWheel, { passive: false }))
onBeforeUnmount(() => container.value?.removeEventListener('wheel', onWheel))
</script>

<template>
  <motion.div
    :initial="{ opacity: 0, y: 100 }"
    :animate="{ opacity: 1, y: 0 }"
    :exit="{ opacity: 0, y: 100 }"
    :transition="{ type: 'spring', duration: 0.4, bounce: 0, delay: 0.1 }"
    class="z-10 shrink-0 border-t border-white/10 bg-black/20 backdrop-blur-xl dark:bg-black/30"
  >
    <div ref="container" class="scrollbar-none flex overflow-x-auto" :style="{ gap: `${gap}px`, padding: `${padding}px` }">
      <button
        v-for="(photo, index) in photos"
        :key="photo.id"
        type="button"
        class="relative shrink-0 overflow-hidden rounded-lg border-2 transition-all duration-200"
        :class="index === currentIndex ? 'scale-110 border-white shadow-lg' : 'border-white/20 grayscale-[.5] hover:border-white/40 hover:grayscale-0'"
        :style="{ width: `${size}px`, height: `${size}px` }"
        :aria-label="photo.title || $t('ui.photo.altFallback')"
        @click="emit('indexChange', index)"
      >
        <PhotoProgressiveImage :src="photo.thumbnailUrl" :alt="photo.title || $t('ui.photo.altFallback')" class="absolute inset-0 h-full w-full" loading="lazy" fit="cover" />
      </button>
    </div>
  </motion.div>
</template>

<style scoped>
.scrollbar-none { scrollbar-width: none; }
.scrollbar-none::-webkit-scrollbar { display: none; }
</style>
