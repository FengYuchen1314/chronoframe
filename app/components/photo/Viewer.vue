<script setup lang="ts">
import { AnimatePresence, motion } from 'motion-v'
import type { GalleryPhoto } from '~~/shared/types/photo'

const props = defineProps<{
  photos: GalleryPhoto[]
  currentIndex: number
  isOpen: boolean
}>()
const emit = defineEmits<{ close: []; indexChange: [number] }>()

type GestureMode = 'idle' | 'pending' | 'horizontal' | 'vertical' | 'zoom-pan' | 'blocked'

const isMobile = useMediaQuery('(max-width: 767px)')
const viewerPane = ref<HTMLElement>()
const paneSize = reactive({ width: 0, height: 0 })
const scale = ref(1)
const panX = ref(0)
const panY = ref(0)
const dragX = ref(0)
const dragY = ref(0)
const trackAnimating = ref(false)
const dismissAnimating = ref(false)
const isSliding = ref(false)
const closeRequested = ref(false)
const naturalSize = reactive({ width: 0, height: 0 })
const gestureMode = ref<GestureMode>('idle')
const gestureStart = reactive({ x: 0, y: 0, time: 0, panX: 0, panY: 0 })
const gestureLast = reactive({ x: 0, y: 0, time: 0 })
const lastTap = reactive({ x: 0, y: 0, time: 0 })
let previousBodyOverflow = ''
let desktopClickTimer: ReturnType<typeof setTimeout> | null = null
let mobileTapTimer: ReturnType<typeof setTimeout> | null = null
let motionTimer: ReturnType<typeof setTimeout> | null = null

const currentPhoto = computed(() => props.photos[props.currentIndex])
const previousPhoto = computed(() => props.currentIndex > 0 ? props.photos[props.currentIndex - 1] : null)
const nextPhoto = computed(() => props.currentIndex < props.photos.length - 1 ? props.photos[props.currentIndex + 1] : null)

const imageAspectRatio = computed(() => {
  if (naturalSize.width > 0 && naturalSize.height > 0) return naturalSize.width / naturalSize.height
  return currentPhoto.value?.aspectRatio || 1
})
const fittedImageSize = computed(() => {
  const { width, height } = paneSize
  if (!width || !height) return { width, height }
  const ratio = imageAspectRatio.value
  if (width / height > ratio) return { width: height * ratio, height }
  return { width, height: width / ratio }
})
const hasBlackSideBars = computed(() => paneSize.width - fittedImageSize.value.width > 3)
const backdropOpacity = computed(() => Math.max(0.18, 1 - Math.abs(dragY.value) / Math.max(paneSize.height * 0.72, 1)))
const mobileStageStyle = computed(() => ({
  transform: `translate3d(0, ${dragY.value}px, 0) scale(${1 - Math.min(Math.abs(dragY.value) / Math.max(paneSize.height, 1), 0.08)})`,
  transition: dismissAnimating.value ? 'transform 220ms cubic-bezier(.2,.8,.2,1)' : 'none',
}))
const mobileTrackStyle = computed(() => ({
  transform: `translate3d(calc(-33.333333% + ${dragX.value}px), 0, 0)`,
  transition: trackAnimating.value ? 'transform 240ms cubic-bezier(.2,.8,.2,1)' : 'none',
}))
const mobileImageStyle = computed(() => ({
  transform: `translate3d(${panX.value}px, ${panY.value}px, 0) scale(${scale.value})`,
  transition: gestureMode.value === 'zoom-pan' ? 'none' : 'transform 220ms cubic-bezier(.2,.8,.2,1)',
}))

const clearTimer = (timer: ReturnType<typeof setTimeout> | null) => {
  if (timer) clearTimeout(timer)
}
const clearInteractionTimers = () => {
  clearTimer(desktopClickTimer)
  clearTimer(mobileTapTimer)
  clearTimer(motionTimer)
  desktopClickTimer = null
  mobileTapTimer = null
  motionTimer = null
}
const resetTransform = () => {
  scale.value = 1
  panX.value = 0
  panY.value = 0
  dragX.value = 0
  dragY.value = 0
  trackAnimating.value = false
  dismissAnimating.value = false
  gestureMode.value = 'idle'
}
const close = () => {
  if (!props.isOpen || closeRequested.value || isSliding.value) return
  closeRequested.value = true
  clearInteractionTimers()
  emit('close')
}
const move = (delta: number) => {
  if (props.photos.length < 2 || isSliding.value) return
  const index = Math.min(props.photos.length - 1, Math.max(0, props.currentIndex + delta))
  if (index === props.currentIndex) return
  resetTransform()
  emit('indexChange', index)
}

const onKeydown = (event: KeyboardEvent) => {
  if (!props.isOpen) return
  if (event.key === 'Escape') {
    event.preventDefault()
    event.stopPropagation()
    close()
  } else if (event.key === 'ArrowLeft') {
    event.preventDefault()
    move(-1)
  } else if (event.key === 'ArrowRight') {
    event.preventDefault()
    move(1)
  }
}
const onWheel = (event: WheelEvent) => {
  if (isMobile.value) return
  scale.value = Math.min(4, Math.max(1, scale.value + (event.deltaY < 0 ? 0.25 : -0.25)))
  if (scale.value === 1) {
    panX.value = 0
    panY.value = 0
  }
}
const toggleZoom = () => {
  scale.value = scale.value > 1 ? 1 : 2
  if (scale.value === 1) {
    panX.value = 0
    panY.value = 0
  }
}
const onDesktopPaneClick = (event: MouseEvent) => {
  if (isMobile.value || event.button !== 0 || scale.value > 1) return
  clearTimer(desktopClickTimer)
  desktopClickTimer = setTimeout(() => {
    desktopClickTimer = null
    if (scale.value === 1) close()
  }, 250)
}
const onDesktopDoubleClick = (event: MouseEvent) => {
  if (isMobile.value) return
  event.preventDefault()
  clearTimer(desktopClickTimer)
  desktopClickTimer = null
  toggleZoom()
}

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value))
const clampZoomPan = (x: number, y: number) => {
  const maxX = Math.max(0, (fittedImageSize.value.width * scale.value - paneSize.width) / 2)
  const maxY = Math.max(0, (fittedImageSize.value.height * scale.value - paneSize.height) / 2)
  return { x: clamp(x, -maxX, maxX), y: clamp(y, -maxY, maxY) }
}
const onTouchStart = (event: TouchEvent) => {
  if (event.touches.length !== 1 || isSliding.value) {
    gestureMode.value = 'blocked'
    return
  }
  const touch = event.touches[0]
  if (!touch) return
  const now = performance.now()
  if (now - lastTap.time < 320 && Math.hypot(touch.clientX - lastTap.x, touch.clientY - lastTap.y) < 32) {
    clearTimer(mobileTapTimer)
    mobileTapTimer = null
  }
  gestureMode.value = 'pending'
  Object.assign(gestureStart, { x: touch.clientX, y: touch.clientY, time: now, panX: panX.value, panY: panY.value })
  Object.assign(gestureLast, { x: touch.clientX, y: touch.clientY, time: now })
}
const onTouchMove = (event: TouchEvent) => {
  if (event.touches.length !== 1 || gestureMode.value === 'blocked' || gestureMode.value === 'idle') return
  const touch = event.touches[0]
  if (!touch) return
  const deltaX = touch.clientX - gestureStart.x
  const deltaY = touch.clientY - gestureStart.y
  if (gestureMode.value === 'pending' && Math.hypot(deltaX, deltaY) > 7) {
    if (scale.value > 1) gestureMode.value = 'zoom-pan'
    else if (Math.abs(deltaX) > Math.abs(deltaY) * 1.05) gestureMode.value = 'horizontal'
    else if (deltaY > 0) gestureMode.value = 'vertical'
    else gestureMode.value = 'blocked'
  }
  if (gestureMode.value === 'horizontal') {
    const atStart = props.currentIndex === 0 && deltaX > 0
    const atEnd = props.currentIndex === props.photos.length - 1 && deltaX < 0
    dragX.value = (atStart || atEnd) ? deltaX * 0.22 : deltaX
  } else if (gestureMode.value === 'vertical') {
    dragY.value = Math.max(0, deltaY)
  } else if (gestureMode.value === 'zoom-pan') {
    const next = clampZoomPan(gestureStart.panX + deltaX, gestureStart.panY + deltaY)
    panX.value = next.x
    panY.value = next.y
  }
  if (gestureMode.value === 'horizontal' || gestureMode.value === 'vertical' || gestureMode.value === 'zoom-pan') {
    if (event.cancelable) event.preventDefault()
  }
  Object.assign(gestureLast, { x: touch.clientX, y: touch.clientY, time: performance.now() })
}
const settleHorizontal = (direction: -1 | 1 | 0) => {
  trackAnimating.value = true
  isSliding.value = direction !== 0
  dragX.value = direction === 0 ? 0 : (direction > 0 ? -paneSize.width : paneSize.width)
  clearTimer(motionTimer)
  motionTimer = setTimeout(() => {
    trackAnimating.value = false
    dragX.value = 0
    if (direction !== 0) {
      scale.value = 1
      panX.value = 0
      panY.value = 0
      emit('indexChange', props.currentIndex + direction)
    }
    isSliding.value = false
    motionTimer = null
  }, 245)
}
const settleVertical = () => {
  dismissAnimating.value = true
  dragY.value = 0
  clearTimer(motionTimer)
  motionTimer = setTimeout(() => {
    dismissAnimating.value = false
    motionTimer = null
  }, 225)
}
const handleMobileTap = (touch: Touch) => {
  const now = performance.now()
  const isDoubleTap = now - lastTap.time < 300
    && Math.hypot(touch.clientX - lastTap.x, touch.clientY - lastTap.y) < 32
  if (isDoubleTap) {
    clearTimer(mobileTapTimer)
    mobileTapTimer = null
    lastTap.time = 0
    toggleZoom()
    return
  }
  Object.assign(lastTap, { x: touch.clientX, y: touch.clientY, time: now })
  clearTimer(mobileTapTimer)
  mobileTapTimer = setTimeout(() => {
    mobileTapTimer = null
    if (scale.value === 1) close()
  }, 260)
}
const onTouchEnd = (event: TouchEvent) => {
  const touch = event.changedTouches[0]
  const mode = gestureMode.value
  gestureMode.value = 'idle'
  if (!touch || mode === 'blocked' || mode === 'idle') return
  const now = performance.now()
  const elapsed = Math.max(now - gestureStart.time, 1)
  const deltaX = touch.clientX - gestureStart.x
  const deltaY = touch.clientY - gestureStart.y
  const recentElapsed = Math.max(now - gestureLast.time, 1)
  const velocityX = (touch.clientX - gestureLast.x) / recentElapsed
  const velocityY = (touch.clientY - gestureLast.y) / recentElapsed

  if (mode === 'pending') {
    handleMobileTap(touch)
  } else if (mode === 'horizontal') {
    const direction = deltaX < 0 ? 1 : -1
    const hasDestination = direction > 0 ? props.currentIndex < props.photos.length - 1 : props.currentIndex > 0
    const shouldMove = hasDestination
      && (Math.abs(deltaX) > paneSize.width * 0.18 || Math.abs(velocityX) > 0.5 || Math.abs(deltaX / elapsed) > 0.42)
    settleHorizontal(shouldMove ? direction : 0)
  } else if (mode === 'vertical') {
    const shouldClose = deltaY > paneSize.height * 0.16 || velocityY > 0.55 || deltaY / elapsed > 0.42
    if (shouldClose) close()
    else settleVertical()
  }
}
const onTouchCancel = () => {
  gestureMode.value = 'idle'
  if (dragX.value) settleHorizontal(0)
  if (dragY.value) settleVertical()
}
const onCurrentImageLoad = (event: Event) => {
  const image = event.currentTarget as HTMLImageElement
  naturalSize.width = image.naturalWidth
  naturalSize.height = image.naturalHeight
}

useResizeObserver(viewerPane, entries => {
  const rect = entries[0]?.contentRect
  if (!rect) return
  paneSize.width = rect.width
  paneSize.height = rect.height
})
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
    clearInteractionTimers()
    resetTransform()
  }
})
watch(() => props.currentIndex, () => {
  resetTransform()
  naturalSize.width = currentPhoto.value?.width || 0
  naturalSize.height = currentPhoto.value?.height || 0
})
onBeforeUnmount(() => {
  clearInteractionTimers()
  if (import.meta.client) document.body.style.overflow = previousBodyOverflow
})
</script>

<template>
  <Teleport to="body">
    <AnimatePresence>
      <motion.div
        v-if="isOpen && currentPhoto"
        class="fixed inset-0 z-[100] h-[100dvh] overflow-hidden bg-black text-white"
        :initial="{ opacity: 0 }"
        :animate="{ opacity: 1 }"
        :exit="{ opacity: 0 }"
        :transition="{ duration: 0.22 }"
        :style="{ backgroundColor: `rgb(0 0 0 / ${backdropOpacity})` }"
      >
        <div
          ref="viewerPane"
          class="relative h-full w-full overflow-hidden"
          @click="onDesktopPaneClick"
          @dblclick="onDesktopDoubleClick"
          @wheel.prevent="onWheel"
        >
          <motion.img
            :key="`desktop-${currentPhoto.id}`"
            data-viewer-current="true"
            :src="currentPhoto.originalUrl"
            :alt="currentPhoto.title || $t('ui.photo.altFallback')"
            class="hidden h-full w-full select-none object-contain md:block"
            :initial="{ opacity: 0.2, scale: 0.985 }"
            :animate="{ opacity: 1, scale }"
            :transition="{ duration: 0.23, ease: [0.2, 0.8, 0.2, 1] }"
            draggable="false"
            @load="onCurrentImageLoad"
          />

          <div
            class="absolute inset-0 overflow-hidden md:hidden"
            style="touch-action: none"
            :style="mobileStageStyle"
            @touchstart="onTouchStart"
            @touchmove="onTouchMove"
            @touchend.prevent="onTouchEnd"
            @touchcancel="onTouchCancel"
          >
            <div class="flex h-full w-[300%] will-change-transform" :style="mobileTrackStyle">
              <div class="grid h-full w-1/3 shrink-0 place-items-center">
                <img v-if="previousPhoto" :src="previousPhoto.originalUrl" :alt="previousPhoto.title || $t('ui.photo.altFallback')" class="h-full w-full select-none object-contain" draggable="false" />
              </div>
              <div class="grid h-full w-1/3 shrink-0 place-items-center">
                <img
                  :key="`mobile-${currentPhoto.id}`"
                  data-viewer-current="true"
                  :src="currentPhoto.originalUrl"
                  :alt="currentPhoto.title || $t('ui.photo.altFallback')"
                  class="h-full w-full select-none object-contain will-change-transform"
                  :style="mobileImageStyle"
                  draggable="false"
                  @load="onCurrentImageLoad"
                />
              </div>
              <div class="grid h-full w-1/3 shrink-0 place-items-center">
                <img v-if="nextPhoto" :src="nextPhoto.originalUrl" :alt="nextPhoto.title || $t('ui.photo.altFallback')" class="h-full w-full select-none object-contain" draggable="false" />
              </div>
            </div>
          </div>

          <div class="viewer-top pointer-events-none absolute inset-x-0 top-0 z-40 bg-linear-to-b from-black/80 via-black/35 to-transparent px-3 pb-16 transition-opacity sm:px-6" :class="dragY ? 'opacity-30' : 'opacity-100'">
            <div class="flex items-start justify-between gap-4">
              <h2 class="min-w-0 truncate pt-2 text-sm font-semibold sm:text-lg">{{ currentPhoto.title || $t('ui.photo.untitled') }}</h2>
              <div class="pointer-events-auto flex shrink-0 items-center gap-1 rounded-full bg-black/35 p-1 backdrop-blur-xl" @click.stop @dblclick.stop @touchstart.stop @touchend.stop>
                <a :href="currentPhoto.originalUrl" :download="currentPhoto.title" class="grid size-11 place-items-center rounded-full transition active:bg-white/20 md:size-10 md:hover:bg-white/15" :title="$t('ui.action.share.actions.downloadOriginal')"><Icon name="tabler:download" class="size-5" /></a>
                <button class="grid size-11 place-items-center rounded-full transition active:bg-white/20 md:size-10 md:hover:bg-white/15" type="button" :aria-label="$t('viewer.navigation.close')" @click.stop="close"><Icon name="tabler:x" class="size-6" /></button>
              </div>
            </div>
          </div>

          <button
            v-if="currentIndex > 0"
            type="button"
            class="absolute inset-y-0 left-0 z-30 hidden w-16 place-items-center border-r text-white/80 transition focus-visible:text-white md:grid lg:w-24"
            :class="hasBlackSideBars ? 'border-white/15 bg-neutral-500/45 backdrop-blur-xl hover:bg-neutral-400/55' : 'border-white/5 bg-linear-to-r from-black/35 to-transparent hover:from-black/70'"
            :aria-label="$t('viewer.navigation.previous')"
            @click.stop="move(-1)"
            @dblclick.stop
          ><Icon name="tabler:chevron-left" class="size-10 drop-shadow-lg lg:size-12" /></button>
          <button
            v-if="currentIndex < photos.length - 1"
            type="button"
            class="absolute inset-y-0 right-0 z-30 hidden w-16 place-items-center border-l text-white/80 transition focus-visible:text-white md:grid lg:w-24"
            :class="hasBlackSideBars ? 'border-white/15 bg-neutral-500/45 backdrop-blur-xl hover:bg-neutral-400/55' : 'border-white/5 bg-linear-to-l from-black/35 to-transparent hover:from-black/70'"
            :aria-label="$t('viewer.navigation.next')"
            @click.stop="move(1)"
            @dblclick.stop
          ><Icon name="tabler:chevron-right" class="size-10 drop-shadow-lg lg:size-12" /></button>

          <div class="pointer-events-none absolute bottom-24 left-1/2 z-30 hidden -translate-x-1/2 rounded-full bg-black/35 px-3 py-1 text-xs text-white/75 backdrop-blur md:block">
            {{ currentIndex + 1 }} / {{ photos.length }}<span v-if="scale > 1" class="ml-2">{{ Math.round(scale * 100) }}%</span>
          </div>
          <div class="viewer-mobile-counter pointer-events-none absolute bottom-0 left-1/2 z-40 -translate-x-1/2 rounded-full bg-black/35 px-3 py-1 text-xs text-white/75 backdrop-blur md:hidden" :class="dragY ? 'opacity-0' : 'opacity-100'">
            {{ currentIndex + 1 }} / {{ photos.length }}<span v-if="scale > 1" class="ml-2">{{ Math.round(scale * 100) }}%</span>
          </div>

          <PhotoGalleryThumbnail class="absolute inset-x-0 bottom-0 z-20 hidden md:block" :photos="photos" :current-index="currentIndex" @click.stop @dblclick.stop @index-change="emit('indexChange', $event)" />
        </div>
      </motion.div>
    </AnimatePresence>
  </Teleport>
</template>

<style scoped>
.viewer-top { padding-top: max(0.75rem, env(safe-area-inset-top)); }
.viewer-mobile-counter { bottom: max(0.75rem, env(safe-area-inset-bottom)); }
</style>
