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
const closeRequested = ref(false)
const touchStart = ref<{ x: number; y: number } | null>(null)
const currentPhoto = computed(() => props.photos[props.currentIndex])
let previousBodyOverflow = ''

const move = (delta: number) => {
  if (props.photos.length < 2) return
  const next = Math.min(props.photos.length - 1, Math.max(0, props.currentIndex + delta))
  if (next === props.currentIndex) return
  scale.value = 1
  emit('indexChange', next)
}
const close = () => {
  if (!props.isOpen || closeRequested.value) return
  closeRequested.value = true
  emit('close')
}
const onKeydown = (event: KeyboardEvent) => {
  if (!props.isOpen) return
  if (event.key === 'Escape') {
    event.preventDefault()
    event.stopPropagation()
    close()
  }
  if (event.key === 'ArrowLeft') {
    event.preventDefault()
    move(-1)
  }
  if (event.key === 'ArrowRight') {
    event.preventDefault()
    move(1)
  }
}
const onWheel = (event: WheelEvent) => {
  scale.value = Math.min(4, Math.max(1, scale.value + (event.deltaY < 0 ? 0.2 : -0.2)))
}
const onTouchStart = (event: TouchEvent) => {
  if (event.touches.length !== 1 || scale.value > 1) {
    touchStart.value = null
    return
  }
  const touch = event.touches[0]
  if (touch) touchStart.value = { x: touch.clientX, y: touch.clientY }
}
const onTouchEnd = (event: TouchEvent) => {
  if (!touchStart.value) return
  const touch = event.changedTouches[0]
  if (!touch) {
    touchStart.value = null
    return
  }
  const deltaX = touch.clientX - touchStart.value.x
  const deltaY = touch.clientY - touchStart.value.y
  if (Math.abs(deltaX) > 52 && Math.abs(deltaX) > Math.abs(deltaY) * 1.25) {
    if (event.cancelable) event.preventDefault()
    move(deltaX > 0 ? -1 : 1)
  }
  touchStart.value = null
}

useEventListener('keydown', onKeydown)
watch(() => props.isOpen, open => {
  closeRequested.value = false
  if (import.meta.client) {
    if (open) {
      previousBodyOverflow = document.body.style.overflow
      document.body.style.overflow = 'hidden'
    } else {
      document.body.style.overflow = previousBodyOverflow
    }
  }
  if (!open) {
    scale.value = 1
    touchStart.value = null
  }
})
watch(() => props.currentIndex, () => { scale.value = 1 })
onBeforeUnmount(() => { if (import.meta.client) document.body.style.overflow = previousBodyOverflow })
</script>

<template>
  <Teleport to="body">
    <AnimatePresence>
      <motion.div
        v-if="isOpen && currentPhoto"
        class="fixed inset-0 z-[100] h-[100dvh] bg-black text-white"
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
                class="h-full w-full touch-none select-none object-contain md:touch-auto"
                :initial="{ opacity: 0.2, scale: 0.985 }"
                :animate="{ opacity: 1, scale }"
                :transition="{ duration: 0.25 }"
                draggable="false"
                @dblclick="scale = scale > 1 ? 1 : 2"
                @touchstart="onTouchStart"
                @touchend="onTouchEnd"
                @touchcancel="touchStart = null"
              />

              <div class="viewer-top pointer-events-none absolute inset-x-0 top-0 z-30 bg-linear-to-b from-black/80 via-black/35 to-transparent px-3 pb-16 transition-opacity sm:px-6" :class="controlsVisible ? 'opacity-100' : 'opacity-0'">
                <div class="flex items-start justify-between gap-4">
                  <div class="min-w-0">
                    <h2 class="truncate pt-2 text-sm font-semibold sm:text-lg">{{ currentPhoto.title || $t('ui.photo.untitled') }}</h2>
                  </div>
                  <div class="pointer-events-auto flex shrink-0 items-center gap-1 rounded-full bg-black/35 p-1 backdrop-blur-xl">
                    <a :href="currentPhoto.originalUrl" :download="currentPhoto.title" class="grid size-11 place-items-center rounded-full transition active:bg-white/20 md:size-10 md:hover:bg-white/15" :title="$t('ui.action.share.actions.downloadOriginal')"><Icon name="tabler:download" class="size-5" /></a>
                    <button class="grid size-11 place-items-center rounded-full transition active:bg-white/20 md:size-10 md:hover:bg-white/15" type="button" :aria-label="$t('viewer.navigation.close')" @click.stop="close"><Icon name="tabler:x" class="size-6" /></button>
                  </div>
                </div>
              </div>

              <button v-if="currentIndex > 0" type="button" class="absolute inset-y-0 left-0 z-20 hidden w-16 place-items-center border-r border-white/5 bg-linear-to-r from-black/35 to-transparent text-white/75 transition hover:from-black/70 hover:text-white focus-visible:from-black/70 focus-visible:text-white md:grid lg:w-24" :aria-label="$t('viewer.navigation.previous')" @click.stop="move(-1)"><Icon name="tabler:chevron-left" class="size-10 drop-shadow-lg lg:size-12" /></button>
              <button v-if="currentIndex < photos.length - 1" type="button" class="absolute inset-y-0 right-0 z-20 hidden w-16 place-items-center border-l border-white/5 bg-linear-to-l from-black/35 to-transparent text-white/75 transition hover:from-black/70 hover:text-white focus-visible:from-black/70 focus-visible:text-white md:grid lg:w-24" :aria-label="$t('viewer.navigation.next')" @click.stop="move(1)"><Icon name="tabler:chevron-right" class="size-10 drop-shadow-lg lg:size-12" /></button>

              <div class="pointer-events-none absolute bottom-3 left-1/2 z-30 hidden -translate-x-1/2 rounded-full bg-black/35 px-3 py-1 text-xs text-white/65 backdrop-blur md:block">
                {{ currentIndex + 1 }} / {{ photos.length }}<span v-if="scale > 1" class="ml-2">{{ Math.round(scale * 100) }}%</span>
              </div>

              <div class="viewer-mobile-toolbar absolute inset-x-0 bottom-0 z-30 grid grid-cols-[3rem_1fr_3rem] items-center gap-3 bg-linear-to-t from-black/85 via-black/35 to-transparent px-4 pt-12 md:hidden">
                <button type="button" class="grid size-12 place-items-center rounded-full bg-white/10 text-white transition active:bg-white/25 disabled:opacity-25" :disabled="currentIndex === 0" :aria-label="$t('viewer.navigation.previous')" @click.stop="move(-1)"><Icon name="tabler:chevron-left" class="size-7" /></button>
                <div class="text-center text-sm font-medium text-white/80">{{ currentIndex + 1 }} / {{ photos.length }}</div>
                <button type="button" class="grid size-12 place-items-center rounded-full bg-white/10 text-white transition active:bg-white/25 disabled:opacity-25" :disabled="currentIndex >= photos.length - 1" :aria-label="$t('viewer.navigation.next')" @click.stop="move(1)"><Icon name="tabler:chevron-right" class="size-7" /></button>
              </div>
            </div>
            <PhotoGalleryThumbnail class="hidden md:block" :photos="photos" :current-index="currentIndex" @index-change="emit('indexChange', $event)" />
          </div>
        </div>
      </motion.div>
    </AnimatePresence>
  </Teleport>
</template>

<style scoped>
.viewer-top { padding-top: max(0.75rem, env(safe-area-inset-top)); }
.viewer-mobile-toolbar { padding-bottom: max(0.75rem, env(safe-area-inset-bottom)); }
</style>
