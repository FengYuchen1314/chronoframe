<script setup lang="ts">
import { AnimatePresence, motion } from 'motion-v'
import type { GalleryPhoto } from '~~/shared/types/photo'

const props = defineProps<{
  photos: GalleryPhoto[]
  currentIndex: number
  isOpen: boolean
}>()
const emit = defineEmits<{ close: []; indexChange: [number] }>()

const controlsVisible = ref(true)
const scale = ref(1)
const touchStart = ref<number | null>(null)
const currentPhoto = computed(() => props.photos[props.currentIndex])

const move = (delta: number) => {
  if (props.photos.length < 2) return
  const next = (props.currentIndex + delta + props.photos.length) % props.photos.length
  scale.value = 1
  emit('indexChange', next)
}
const close = () => emit('close')
const onKeydown = (event: KeyboardEvent) => {
  if (!props.isOpen) return
  if (event.key === 'Escape') close()
  if (event.key === 'ArrowLeft') move(-1)
  if (event.key === 'ArrowRight') move(1)
}
const onWheel = (event: WheelEvent) => {
  scale.value = Math.min(4, Math.max(1, scale.value + (event.deltaY < 0 ? 0.2 : -0.2)))
}
const onTouchEnd = (event: TouchEvent) => {
  if (touchStart.value === null) return
  const end = event.changedTouches[0]?.clientX ?? touchStart.value
  if (Math.abs(end - touchStart.value) > 60) move(end > touchStart.value ? -1 : 1)
  touchStart.value = null
}

useEventListener('keydown', onKeydown)
watch(() => props.isOpen, open => {
  if (import.meta.client) document.body.style.overflow = open ? 'hidden' : ''
  if (!open) {
    scale.value = 1
  }
})
watch(() => props.currentIndex, () => { scale.value = 1 })
onBeforeUnmount(() => { if (import.meta.client) document.body.style.overflow = '' })
</script>

<template>
  <Teleport to="body">
    <AnimatePresence>
      <motion.div
        v-if="isOpen && currentPhoto"
        class="fixed inset-0 z-[100] bg-white/50 text-white backdrop-blur-2xl dark:bg-black/50"
        :initial="{ opacity: 0 }"
        :animate="{ opacity: 1 }"
        :exit="{ opacity: 0 }"
        :transition="{ duration: 0.3 }"
        @mousemove="controlsVisible = true"
      >
        <div class="flex h-full w-full">
          <div class="z-10 flex min-h-0 min-w-0 flex-1 flex-col">
            <div class="group relative min-h-0 min-w-0 flex-1 overflow-hidden bg-black/90" @wheel.prevent="onWheel">
              <motion.img
                :key="currentPhoto.id"
                :src="currentPhoto.originalUrl"
                :alt="currentPhoto.title || $t('ui.photo.altFallback')"
                class="h-full w-full select-none object-contain"
                :initial="{ opacity: 0.2, scale: 0.985 }"
                :animate="{ opacity: 1, scale }"
                :transition="{ duration: 0.25 }"
                draggable="false"
                @dblclick="scale = scale > 1 ? 1 : 2"
                @touchstart="touchStart = $event.touches[0]?.clientX ?? null"
                @touchend="onTouchEnd"
              />

              <div class="pointer-events-none absolute inset-x-0 top-0 z-10 bg-linear-to-b from-black/70 to-transparent px-4 pb-16 pt-4 transition-opacity" :class="controlsVisible ? 'opacity-100' : 'opacity-0'">
                <div class="flex items-start justify-between gap-4">
                  <div class="min-w-0">
                    <h2 class="truncate text-base font-semibold sm:text-lg">{{ currentPhoto.title || $t('ui.photo.untitled') }}</h2>
                  </div>
                  <div class="pointer-events-auto flex shrink-0 items-center gap-1 rounded-full bg-black/25 p-1 backdrop-blur-xl">
                    <a :href="currentPhoto.originalUrl" :download="currentPhoto.title" class="grid size-9 place-items-center rounded-full transition hover:bg-white/15" :title="$t('ui.action.share.actions.downloadOriginal')"><Icon name="tabler:download" class="size-5" /></a>
                    <button class="grid size-9 place-items-center rounded-full transition hover:bg-white/15" type="button" :aria-label="$t('viewer.navigation.close')" @click="close"><Icon name="tabler:x" class="size-5" /></button>
                  </div>
                </div>
              </div>

              <button v-if="currentIndex > 0" type="button" class="absolute left-4 top-1/2 z-20 grid size-9 -translate-y-1/2 place-items-center rounded-full bg-black/30 opacity-0 backdrop-blur-sm transition group-hover:opacity-100 hover:bg-black/45" :aria-label="$t('viewer.navigation.previous')" @click="move(-1)"><Icon name="tabler:chevron-left" class="size-6" /></button>
              <button v-if="currentIndex < photos.length - 1" type="button" class="absolute right-4 top-1/2 z-20 grid size-9 -translate-y-1/2 place-items-center rounded-full bg-black/30 opacity-0 backdrop-blur-sm transition group-hover:opacity-100 hover:bg-black/45" :aria-label="$t('viewer.navigation.next')" @click="move(1)"><Icon name="tabler:chevron-right" class="size-6" /></button>

              <div class="pointer-events-none absolute bottom-3 left-1/2 z-10 -translate-x-1/2 rounded-full bg-black/35 px-3 py-1 text-xs text-white/65 backdrop-blur">
                {{ currentIndex + 1 }} / {{ photos.length }}<span v-if="scale > 1" class="ml-2">{{ Math.round(scale * 100) }}%</span>
              </div>
            </div>
            <PhotoGalleryThumbnail :photos="photos" :current-index="currentIndex" @index-change="emit('indexChange', $event)" />
          </div>
        </div>
      </motion.div>
    </AnimatePresence>
  </Teleport>
</template>
