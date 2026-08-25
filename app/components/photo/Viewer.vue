<script setup lang="ts">
import { AnimatePresence, motion } from 'motion-v'
import type { GalleryPhoto } from '~~/shared/types/photo'

const props = defineProps<{
  photos: GalleryPhoto[]
  currentIndex: number
  isOpen: boolean
}>()
const emit = defineEmits<{ close: []; indexChange: [number] }>()

type GestureMode = 'idle' | 'pending' | 'horizontal' | 'vertical' | 'zoom-pan' | 'pinch' | 'blocked'
type PointerPoint = { x: number; y: number }

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
const zoomSettling = ref(false)
const isSliding = ref(false)
const slideDuration = ref(240)
const closeRequested = ref(false)
const naturalSize = reactive({ width: 0, height: 0 })
const gestureMode = ref<GestureMode>('idle')
const gestureStart = reactive({ x: 0, y: 0, time: 0, panX: 0, panY: 0 })
const gestureLast = reactive({ x: 0, y: 0, time: 0 })
const pinchStart = reactive({ distance: 1, scale: 1, centerX: 0, centerY: 0, panX: 0, panY: 0 })
const lastTap = reactive({ x: 0, y: 0, time: 0 })
const visualIndex = ref(props.currentIndex)
const showGestureHint = ref(false)
const activePointers = new Map<number, PointerPoint>()
let previousBodyOverflow = ''
let desktopClickTimer: ReturnType<typeof setTimeout> | null = null
let mobileTapTimer: ReturnType<typeof setTimeout> | null = null
let motionTimer: ReturnType<typeof setTimeout> | null = null
let hintTimer: ReturnType<typeof setTimeout> | null = null
let zoomTimer: ReturnType<typeof setTimeout> | null = null
let gesturePointerId: number | null = null
let dragFrame: number | null = null
let pendingDragX = 0

const currentPhoto = computed(() => props.photos[props.currentIndex])
const mobileSlides = computed(() => {
  const start = Math.max(0, visualIndex.value - 2)
  const end = Math.min(props.photos.length - 1, visualIndex.value + 2)
  const result: Array<{ photo: GalleryPhoto; index: number }> = []
  for (let index = start; index <= end; index++) {
    const photo = props.photos[index]
    if (photo) result.push({ photo, index })
  }
  return result
})

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
  transition: dismissAnimating.value ? 'transform 210ms cubic-bezier(.22,.8,.2,1)' : 'none',
}))
const mobileTrackStyle = computed(() => ({
  transform: `translate3d(calc(${-visualIndex.value * 100}% + ${dragX.value}px), 0, 0)`,
  transition: trackAnimating.value ? `transform ${slideDuration.value}ms cubic-bezier(.22,.82,.2,1)` : 'none',
}))
const mobileImageStyle = computed(() => ({
  transform: `translate3d(${panX.value}px, ${panY.value}px, 0) scale(${scale.value})`,
  transition: gestureMode.value === 'pinch' || gestureMode.value === 'zoom-pan' || !zoomSettling.value
    ? 'none'
    : 'transform 210ms cubic-bezier(.22,.82,.2,1)',
}))

const clearTimer = (timer: ReturnType<typeof setTimeout> | null) => { if (timer) clearTimeout(timer) }
const clearDragFrame = () => {
  if (dragFrame !== null) cancelAnimationFrame(dragFrame)
  dragFrame = null
}
const clearInteractionTimers = () => {
  clearTimer(desktopClickTimer)
  clearTimer(mobileTapTimer)
  clearTimer(motionTimer)
  clearTimer(hintTimer)
  clearTimer(zoomTimer)
  clearDragFrame()
  desktopClickTimer = null
  mobileTapTimer = null
  motionTimer = null
  hintTimer = null
  zoomTimer = null
}
const resetTransform = () => {
  scale.value = 1
  panX.value = 0
  panY.value = 0
  dragX.value = 0
  dragY.value = 0
  pendingDragX = 0
  trackAnimating.value = false
  dismissAnimating.value = false
  zoomSettling.value = false
  gestureMode.value = 'idle'
  gesturePointerId = null
  activePointers.clear()
  clearDragFrame()
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
  visualIndex.value = index
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
const settleZoom = () => {
  zoomSettling.value = true
  clearTimer(zoomTimer)
  zoomTimer = setTimeout(() => {
    zoomSettling.value = false
    zoomTimer = null
  }, 220)
}
const toggleZoom = () => {
  zoomSettling.value = true
  scale.value = scale.value > 1 ? 1 : 2
  if (scale.value === 1) {
    panX.value = 0
    panY.value = 0
  }
  settleZoom()
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
  window.getSelection()?.removeAllRanges()
  clearTimer(desktopClickTimer)
  desktopClickTimer = null
  toggleZoom()
}

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value))
const clampZoomPanForScale = (x: number, y: number, nextScale: number) => {
  const maxX = Math.max(0, (fittedImageSize.value.width * nextScale - paneSize.width) / 2)
  const maxY = Math.max(0, (fittedImageSize.value.height * nextScale - paneSize.height) / 2)
  return { x: clamp(x, -maxX, maxX), y: clamp(y, -maxY, maxY) }
}
const clampZoomPan = (x: number, y: number) => clampZoomPanForScale(x, y, scale.value)
const pointerPair = () => Array.from(activePointers.values()).slice(0, 2)
const pairMetrics = (points: PointerPoint[]) => {
  const [first, second] = points
  if (!first || !second) return null
  return {
    distance: Math.max(1, Math.hypot(second.x - first.x, second.y - first.y)),
    centerX: (first.x + second.x) / 2,
    centerY: (first.y + second.y) / 2,
  }
}
const beginPinch = () => {
  const metrics = pairMetrics(pointerPair())
  if (!metrics) return
  clearTimer(mobileTapTimer)
  mobileTapTimer = null
  lastTap.time = 0
  gestureMode.value = 'pinch'
  zoomSettling.value = false
  clearDragFrame()
  pendingDragX = 0
  dragX.value = 0
  dragY.value = 0
  Object.assign(pinchStart, { ...metrics, scale: scale.value, panX: panX.value, panY: panY.value })
}
const updatePinch = () => {
  const metrics = pairMetrics(pointerPair())
  if (!metrics) return
  const nextScale = clamp(pinchStart.scale * (metrics.distance / pinchStart.distance), 1, 4)
  const contentX = (pinchStart.centerX - paneSize.width / 2 - pinchStart.panX) / pinchStart.scale
  const contentY = (pinchStart.centerY - paneSize.height / 2 - pinchStart.panY) / pinchStart.scale
  const desiredX = metrics.centerX - paneSize.width / 2 - contentX * nextScale
  const desiredY = metrics.centerY - paneSize.height / 2 - contentY * nextScale
  const nextPan = clampZoomPanForScale(desiredX, desiredY, nextScale)
  scale.value = nextScale
  panX.value = nextPan.x
  panY.value = nextPan.y
}
const scheduleDragX = (value: number) => {
  pendingDragX = value
  if (dragFrame !== null) return
  dragFrame = requestAnimationFrame(() => {
    dragX.value = pendingDragX
    dragFrame = null
  })
}
const flushDragX = () => {
  clearDragFrame()
  dragX.value = pendingDragX
}
const onPointerStart = (event: PointerEvent) => {
  if ((event.pointerType === 'mouse' && event.button !== 0) || isSliding.value || trackAnimating.value || dismissAnimating.value || activePointers.size >= 2) return
  activePointers.set(event.pointerId, { x: event.clientX, y: event.clientY })
  ;(event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId)
  if (activePointers.size === 2) {
    beginPinch()
    return
  }

  gesturePointerId = event.pointerId
  const now = performance.now()
  if (now - lastTap.time < 320 && Math.hypot(event.clientX - lastTap.x, event.clientY - lastTap.y) < 32) {
    clearTimer(mobileTapTimer)
    mobileTapTimer = null
  }
  gestureMode.value = 'pending'
  zoomSettling.value = false
  Object.assign(gestureStart, { x: event.clientX, y: event.clientY, time: now, panX: panX.value, panY: panY.value })
  Object.assign(gestureLast, { x: event.clientX, y: event.clientY, time: now })
}
const onPointerMove = (event: PointerEvent) => {
  if (!activePointers.has(event.pointerId)) return
  activePointers.set(event.pointerId, { x: event.clientX, y: event.clientY })
  if (gestureMode.value === 'pinch' && activePointers.size >= 2) {
    updatePinch()
    if (event.cancelable) event.preventDefault()
    return
  }
  if (event.pointerId !== gesturePointerId || gestureMode.value === 'blocked' || gestureMode.value === 'idle') return

  const deltaX = event.clientX - gestureStart.x
  const deltaY = event.clientY - gestureStart.y
  if (gestureMode.value === 'pending' && Math.hypot(deltaX, deltaY) > 6) {
    if (scale.value > 1) gestureMode.value = 'zoom-pan'
    else if (Math.abs(deltaX) > Math.abs(deltaY) * 1.03) gestureMode.value = 'horizontal'
    else if (deltaY > 0) gestureMode.value = 'vertical'
    else gestureMode.value = 'blocked'
  }
  if (gestureMode.value === 'horizontal') {
    const atStart = visualIndex.value === 0 && deltaX > 0
    const atEnd = visualIndex.value === props.photos.length - 1 && deltaX < 0
    scheduleDragX((atStart || atEnd) ? deltaX * 0.2 : deltaX)
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
  Object.assign(gestureLast, { x: event.clientX, y: event.clientY, time: performance.now() })
}
const settleHorizontal = (direction: -1 | 1 | 0, velocity = 0) => {
  flushDragX()
  const progress = Math.min(1, Math.abs(dragX.value) / Math.max(paneSize.width, 1))
  slideDuration.value = direction === 0
    ? 190
    : Math.round(clamp(270 - progress * 85 - Math.abs(velocity) * 45, 155, 270))
  trackAnimating.value = true
  isSliding.value = direction !== 0
  dragX.value = direction === 0 ? 0 : (direction > 0 ? -paneSize.width : paneSize.width)
  pendingDragX = dragX.value
  clearTimer(motionTimer)
  motionTimer = setTimeout(() => {
    if (direction !== 0) {
      const nextIndex = props.currentIndex + direction
      visualIndex.value = nextIndex
      dragX.value = 0
      pendingDragX = 0
      scale.value = 1
      panX.value = 0
      panY.value = 0
      emit('indexChange', nextIndex)
    } else {
      dragX.value = 0
      pendingDragX = 0
    }
    trackAnimating.value = false
    isSliding.value = false
    motionTimer = null
  }, slideDuration.value + 16)
}
const settleVertical = () => {
  dismissAnimating.value = true
  dragY.value = 0
  clearTimer(motionTimer)
  motionTimer = setTimeout(() => {
    dismissAnimating.value = false
    motionTimer = null
  }, 220)
}
const handleMobileTap = (event: PointerEvent) => {
  const now = performance.now()
  const isDoubleTap = now - lastTap.time < 300
    && Math.hypot(event.clientX - lastTap.x, event.clientY - lastTap.y) < 32
  if (isDoubleTap) {
    clearTimer(mobileTapTimer)
    mobileTapTimer = null
    lastTap.time = 0
    toggleZoom()
    return
  }
  Object.assign(lastTap, { x: event.clientX, y: event.clientY, time: now })
  clearTimer(mobileTapTimer)
  mobileTapTimer = setTimeout(() => {
    mobileTapTimer = null
    if (scale.value === 1) close()
  }, 260)
}
const onPointerEnd = (event: PointerEvent) => {
  if (!activePointers.has(event.pointerId)) return
  activePointers.delete(event.pointerId)
  if (gestureMode.value === 'pinch') {
    const remaining = Array.from(activePointers.entries())[0]
    if (remaining) {
      gesturePointerId = remaining[0]
      gestureMode.value = scale.value > 1 ? 'zoom-pan' : 'blocked'
      Object.assign(gestureStart, { x: remaining[1].x, y: remaining[1].y, time: performance.now(), panX: panX.value, panY: panY.value })
    } else {
      gesturePointerId = null
      gestureMode.value = 'idle'
      const next = clampZoomPan(panX.value, panY.value)
      panX.value = next.x
      panY.value = next.y
      settleZoom()
    }
    return
  }
  if (event.pointerId !== gesturePointerId) return
  gesturePointerId = null
  const mode = gestureMode.value
  gestureMode.value = 'idle'
  if (mode === 'blocked' || mode === 'idle') return
  const now = performance.now()
  const elapsed = Math.max(now - gestureStart.time, 1)
  const deltaX = event.clientX - gestureStart.x
  const deltaY = event.clientY - gestureStart.y
  const recentElapsed = Math.max(now - gestureLast.time, 1)
  const velocityX = (event.clientX - gestureLast.x) / recentElapsed
  const velocityY = (event.clientY - gestureLast.y) / recentElapsed

  if (mode === 'pending') {
    handleMobileTap(event)
  } else if (mode === 'horizontal') {
    const direction = deltaX < 0 ? 1 : -1
    const hasDestination = direction > 0 ? props.currentIndex < props.photos.length - 1 : props.currentIndex > 0
    const shouldMove = hasDestination
      && (Math.abs(deltaX) > paneSize.width * 0.16 || Math.abs(velocityX) > 0.42 || Math.abs(deltaX / elapsed) > 0.36)
    settleHorizontal(shouldMove ? direction : 0, velocityX)
  } else if (mode === 'vertical') {
    const shouldClose = deltaY > paneSize.height * 0.15 || velocityY > 0.5 || deltaY / elapsed > 0.38
    if (shouldClose) close()
    else settleVertical()
  } else if (mode === 'zoom-pan') {
    const next = clampZoomPan(panX.value, panY.value)
    panX.value = next.x
    panY.value = next.y
    settleZoom()
  }
}
const onPointerCancel = (event: PointerEvent) => {
  activePointers.delete(event.pointerId)
  if (gestureMode.value === 'pinch' && activePointers.size) {
    const remaining = Array.from(activePointers.entries())[0]!
    gesturePointerId = remaining[0]
    gestureMode.value = scale.value > 1 ? 'zoom-pan' : 'blocked'
    Object.assign(gestureStart, { x: remaining[1].x, y: remaining[1].y, time: performance.now(), panX: panX.value, panY: panY.value })
    return
  }
  if (activePointers.size) return
  gesturePointerId = null
  gestureMode.value = 'idle'
  if (dragX.value || pendingDragX) settleHorizontal(0)
  if (dragY.value) settleVertical()
  if (scale.value > 1) settleZoom()
}
const onCurrentImageLoad = (event: Event) => {
  const image = event.currentTarget as HTMLImageElement
  naturalSize.width = image.naturalWidth
  naturalSize.height = image.naturalHeight
}
const maybeShowGestureHint = () => {
  if (!import.meta.client || !props.isOpen || !isMobile.value || showGestureHint.value) return
  try {
    const key = 'chronoframe-mobile-viewer-gesture-hint-v1'
    if (localStorage.getItem(key)) return
    localStorage.setItem(key, 'shown')
  } catch {
    // Private browsing can deny storage. The hint should still be non-blocking.
  }
  showGestureHint.value = true
  clearTimer(hintTimer)
  hintTimer = setTimeout(() => {
    showGestureHint.value = false
    hintTimer = null
  }, 3600)
}

useResizeObserver(viewerPane, entries => {
  const rect = entries[0]?.contentRect
  if (!rect) return
  paneSize.width = rect.width
  paneSize.height = rect.height
})
useEventListener('keydown', onKeydown)
watch([() => props.isOpen, isMobile], ([open]) => {
  if (open) nextTick(maybeShowGestureHint)
})
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
    showGestureHint.value = false
    clearInteractionTimers()
    resetTransform()
  }
})
watch(() => props.currentIndex, (index) => {
  visualIndex.value = index
  if (!isSliding.value) {
    scale.value = 1
    panX.value = 0
    panY.value = 0
    dragX.value = 0
    pendingDragX = 0
  }
  naturalSize.width = currentPhoto.value?.width || 0
  naturalSize.height = currentPhoto.value?.height || 0
}, { flush: 'sync' })
onBeforeUnmount(() => {
  clearInteractionTimers()
  activePointers.clear()
  if (import.meta.client) document.body.style.overflow = previousBodyOverflow
})
</script>

<template>
  <Teleport to="body">
    <AnimatePresence>
      <motion.div
        v-if="isOpen && currentPhoto"
        class="viewer-no-select fixed inset-0 z-[100] h-[100dvh] overflow-hidden bg-black text-white"
        :initial="{ opacity: 0 }"
        :animate="{ opacity: 1 }"
        :exit="{ opacity: 0 }"
        :transition="{ duration: 0.18 }"
        :style="{ backgroundColor: `rgb(0 0 0 / ${backdropOpacity})` }"
      >
        <div ref="viewerPane" class="relative h-full w-full overflow-hidden" @click="onDesktopPaneClick" @dblclick="onDesktopDoubleClick" @selectstart.prevent @dragstart.prevent @wheel.prevent="onWheel">
          <motion.div
            :key="`desktop-${currentPhoto.id}`"
            data-viewer-current="true"
            class="hidden h-full w-full md:block"
            :initial="{ opacity: 0.35, scale: 0.992 }"
            :animate="{ opacity: 1, scale }"
            :transition="{ duration: 0.2, ease: [0.2, 0.8, 0.2, 1] }"
          >
            <PhotoProgressiveImage :src="currentPhoto.originalUrl" :placeholder-src="currentPhoto.thumbnailUrl" :alt="currentPhoto.title || $t('ui.photo.altFallback')" fit="contain" loading="eager" fetch-priority="high" class="h-full w-full bg-black" @load="onCurrentImageLoad" />
          </motion.div>

          <div class="absolute inset-0 overflow-hidden md:hidden" style="touch-action: none" :style="mobileStageStyle" @pointerdown="onPointerStart" @pointermove="onPointerMove" @pointerup.prevent="onPointerEnd" @pointercancel="onPointerCancel">
            <div class="absolute inset-0 will-change-transform" :style="mobileTrackStyle">
              <div v-for="slide in mobileSlides" :key="slide.photo.id" class="absolute inset-y-0 w-full" :style="{ left: `${slide.index * 100}%` }">
                <PhotoProgressiveImage
                  :data-viewer-current="slide.index === visualIndex ? 'true' : undefined"
                  :src="Math.abs(slide.index - visualIndex) <= 1 ? slide.photo.originalUrl : slide.photo.thumbnailUrl"
                  :placeholder-src="Math.abs(slide.index - visualIndex) <= 1 ? slide.photo.thumbnailUrl : null"
                  :alt="slide.photo.title || $t('ui.photo.altFallback')"
                  fit="contain"
                  :loading="Math.abs(slide.index - visualIndex) <= 1 ? 'eager' : 'lazy'"
                  :fetch-priority="slide.index === visualIndex ? 'high' : 'low'"
                  class="h-full w-full select-none bg-black will-change-transform"
                  :style="slide.index === visualIndex ? mobileImageStyle : undefined"
                  @load="slide.index === visualIndex && onCurrentImageLoad($event)"
                />
              </div>
            </div>
          </div>

          <AnimatePresence>
            <motion.div v-if="showGestureHint" class="viewer-gesture-hint pointer-events-none absolute left-1/2 top-0 z-50 w-[min(88vw,22rem)] -translate-x-1/2 md:hidden" :initial="{ opacity: 0, y: -14, scale: 0.96 }" :animate="{ opacity: 1, y: 0, scale: 1 }" :exit="{ opacity: 0, y: -10, scale: 0.97 }" :transition="{ duration: 0.28 }">
              <div class="rounded-2xl border border-white/20 bg-neutral-700/55 px-5 py-4 text-center shadow-2xl backdrop-blur-2xl">
                <div class="viewer-pinch-demo relative mx-auto mb-2 h-12 w-24">
                  <Icon name="tabler:photo" class="absolute left-1/2 top-1/2 size-8 -translate-x-1/2 -translate-y-1/2 text-white/80" />
                  <span class="viewer-finger viewer-finger-left" />
                  <span class="viewer-finger viewer-finger-right" />
                </div>
                <p class="text-sm font-semibold text-white">双指捏合或张开，调整图片大小</p>
                <p class="mt-1 text-xs text-white/70">左右滑动切换图片 · 向下滑动退出</p>
              </div>
            </motion.div>
          </AnimatePresence>

          <div class="viewer-top pointer-events-none absolute inset-x-0 top-0 z-40 bg-linear-to-b from-black/80 via-black/35 to-transparent px-3 pb-16 transition-opacity sm:px-6" :class="dragY ? 'opacity-30' : 'opacity-100'">
            <div class="flex items-start justify-between gap-4">
              <h2 class="min-w-0 truncate pt-2 text-sm font-semibold sm:text-lg">{{ currentPhoto.title || $t('ui.photo.untitled') }}</h2>
              <div class="pointer-events-auto flex shrink-0 items-center gap-1 rounded-full bg-black/35 p-1 backdrop-blur-xl" @click.stop @dblclick.stop @pointerdown.stop @pointerup.stop>
                <a :href="currentPhoto.originalUrl" :download="currentPhoto.title" class="grid size-11 place-items-center rounded-full transition active:bg-white/20 md:size-10 md:hover:bg-white/15" :title="$t('ui.action.share.actions.downloadOriginal')"><Icon name="tabler:download" class="size-5" /></a>
                <button class="grid size-11 place-items-center rounded-full transition active:bg-white/20 md:size-10 md:hover:bg-white/15" type="button" :aria-label="$t('viewer.navigation.close')" @click.stop="close"><Icon name="tabler:x" class="size-6" /></button>
              </div>
            </div>
          </div>

          <button v-if="currentIndex > 0" type="button" class="absolute inset-y-0 left-0 z-30 hidden w-16 place-items-center border-r text-white/80 transition focus-visible:text-white md:grid lg:w-24" :class="hasBlackSideBars ? 'viewer-nav-frosted border-white/15' : 'border-white/5 bg-linear-to-r from-black/35 to-transparent hover:from-black/70'" :aria-label="$t('viewer.navigation.previous')" @click.stop="move(-1)" @dblclick.stop><Icon name="tabler:chevron-left" class="size-10 drop-shadow-lg lg:size-12" /></button>
          <button v-if="currentIndex < photos.length - 1" type="button" class="absolute inset-y-0 right-0 z-30 hidden w-16 place-items-center border-l text-white/80 transition focus-visible:text-white md:grid lg:w-24" :class="hasBlackSideBars ? 'viewer-nav-frosted border-white/15' : 'border-white/5 bg-linear-to-l from-black/35 to-transparent hover:from-black/70'" :aria-label="$t('viewer.navigation.next')" @click.stop="move(1)" @dblclick.stop><Icon name="tabler:chevron-right" class="size-10 drop-shadow-lg lg:size-12" /></button>

          <div class="pointer-events-none absolute bottom-24 left-1/2 z-30 hidden -translate-x-1/2 rounded-full bg-black/35 px-3 py-1 text-xs text-white/75 backdrop-blur md:block">{{ currentIndex + 1 }} / {{ photos.length }}<span v-if="scale > 1" class="ml-2">{{ Math.round(scale * 100) }}%</span></div>
          <div class="viewer-mobile-counter pointer-events-none absolute bottom-0 left-1/2 z-40 -translate-x-1/2 rounded-full bg-black/35 px-3 py-1 text-xs text-white/75 backdrop-blur md:hidden" :class="dragY ? 'opacity-0' : 'opacity-100'">{{ currentIndex + 1 }} / {{ photos.length }}<span v-if="scale > 1" class="ml-2">{{ Math.round(scale * 100) }}%</span></div>

          <PhotoGalleryThumbnail class="absolute inset-x-0 bottom-0 z-20 hidden md:block" :photos="photos" :current-index="currentIndex" @click.stop @dblclick.stop @index-change="emit('indexChange', $event)" />
        </div>
      </motion.div>
    </AnimatePresence>
  </Teleport>
</template>

<style scoped>
.viewer-top { padding-top: max(0.75rem, env(safe-area-inset-top)); }
.viewer-no-select, .viewer-no-select * { -webkit-user-select: none !important; user-select: none !important; }
.viewer-no-select img { -webkit-user-drag: none; }
.viewer-mobile-counter { bottom: max(0.75rem, env(safe-area-inset-bottom)); }
.viewer-gesture-hint { padding-top: max(5.25rem, calc(env(safe-area-inset-top) + 4.5rem)); }
.viewer-nav-frosted { background-color: rgb(82 82 91 / 62%); -webkit-backdrop-filter: blur(18px); backdrop-filter: blur(18px); }
.viewer-nav-frosted:hover { background-color: rgb(113 113 122 / 72%); }
.viewer-finger { position: absolute; top: 50%; width: 0.85rem; height: 0.85rem; margin-top: -0.425rem; border: 2px solid rgb(255 255 255 / 82%); border-radius: 9999px; box-shadow: 0 0 0 4px rgb(255 255 255 / 10%); }
.viewer-finger-left { animation: pinch-left 1.5s ease-in-out infinite; }
.viewer-finger-right { animation: pinch-right 1.5s ease-in-out infinite; }
@keyframes pinch-left { 0%, 100% { left: 2.25rem; transform: scale(0.92); } 50% { left: 0.55rem; transform: scale(1.08); } }
@keyframes pinch-right { 0%, 100% { right: 2.25rem; transform: scale(0.92); } 50% { right: 0.55rem; transform: scale(1.08); } }
@media (prefers-reduced-motion: reduce) { .viewer-finger-left, .viewer-finger-right { animation: none; } }
</style>
