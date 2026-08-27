<script setup lang="ts">
import type { GalleryPhoto } from '~~/shared/types/photo'

const props = withDefaults(defineProps<{
  photo: GalleryPhoto
  index: number
  selected?: boolean
  selectionMode?: boolean
}>(), { selected: false, selectionMode: false })
const emit = defineEmits<{
  activate: [number, MouseEvent | KeyboardEvent]
  contextAction: [GalleryPhoto, number, number]
  select: [number, MouseEvent | KeyboardEvent]
}>()

const aspectRatio = computed(() => props.photo.aspectRatio || 1.2)
const card = ref<HTMLElement>()
const nearViewport = ref(false)
const { stop: stopObserving } = useIntersectionObserver(card, entries => {
  if (entries.some(entry => entry.isIntersecting)) {
    nearViewport.value = true
    stopObserving()
  }
}, { rootMargin: '320px 0px' })
const camera = computed(() => [props.photo.exif?.Make, props.photo.exif?.Model].filter(Boolean).join(' '))
let longPressTimer: ReturnType<typeof setTimeout> | null = null
let longPressStart = { x: 0, y: 0 }
let suppressNextClick = false

const clearLongPress = () => {
  if (longPressTimer) clearTimeout(longPressTimer)
  longPressTimer = null
}
const onPointerDown = (event: PointerEvent) => {
  if (event.pointerType === 'mouse') return
  clearLongPress()
  longPressStart = { x: event.clientX, y: event.clientY }
  longPressTimer = setTimeout(() => {
    longPressTimer = null
    suppressNextClick = true
    navigator.vibrate?.(18)
    emit('contextAction', props.photo, event.clientX, event.clientY)
  }, 520)
}
const onPointerMove = (event: PointerEvent) => {
  if (Math.hypot(event.clientX - longPressStart.x, event.clientY - longPressStart.y) > 10) clearLongPress()
}
const onActivate = (event: MouseEvent) => {
  if (suppressNextClick) {
    suppressNextClick = false
    event.preventDefault()
    return
  }
  emit('activate', props.index, event)
}
const onKeyboardActivate = (event: KeyboardEvent) => emit('activate', props.index, event)
const onContextMenu = (event: MouseEvent) => {
  event.preventDefault()
  emit('contextAction', props.photo, event.clientX, event.clientY)
}
onBeforeUnmount(clearLongPress)
</script>

<template>
  <div
    ref="card"
    role="button"
    tabindex="0"
    class="group relative block w-full touch-manipulation overflow-hidden bg-neutral-200 text-left transition-all active:opacity-85 dark:bg-neutral-800"
    :class="[selectionMode ? 'cursor-default' : 'cursor-zoom-in', selected && 'ring-4 ring-inset ring-primary']"
    :style="{ aspectRatio }"
    :aria-label="photo.title || $t('ui.photo.altFallback')"
    :aria-pressed="selectionMode ? selected : undefined"
    @click="onActivate"
    @keydown.enter.prevent="onKeyboardActivate"
    @contextmenu.stop="onContextMenu"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="clearLongPress"
    @pointercancel="clearLongPress"
  >
    <PhotoProgressiveImage
      v-if="nearViewport"
      :src="photo.thumbnailUrl"
      :fallback-src="photo.previewUrl"
      :alt="photo.title"
      loading="eager"
      :fetch-priority="index < 8 ? 'high' : 'low'"
      fit="cover"
      class="absolute inset-0 h-full w-full transition-transform duration-500 group-hover:scale-[1.035]"
    />
    <div class="absolute inset-0 bg-black/0 transition-colors duration-300 group-hover:bg-black/15" />
    <div class="absolute inset-x-0 bottom-0 translate-y-full bg-linear-to-t from-black/70 to-transparent p-3 pt-12 text-white opacity-0 transition duration-300 group-hover:translate-y-0 group-hover:opacity-100">
      <p class="truncate text-sm font-semibold">{{ photo.title }}</p>
      <p v-if="photo.dateTaken || photo.city" class="mt-0.5 truncate text-xs text-white/75">
        <span v-if="photo.dateTaken">{{ formatGalleryDate(photo.dateTaken, { year: 'numeric', month: '2-digit', day: '2-digit' }) }}</span>
        <span v-if="photo.city">{{ photo.dateTaken ? ' · ' : '' }}{{ photo.city }}</span>
      </p>
      <p v-if="camera" class="mt-1 truncate text-xs text-white/65">
        <Icon name="tabler:camera" class="mr-1 inline-block size-3.5" />{{ camera }}
      </p>
      <div v-if="photo.tags.length" class="mt-2 flex flex-wrap gap-1">
        <span v-for="tag in photo.tags.slice(0, 4)" :key="tag" class="rounded-full bg-white/15 px-2 py-0.5 text-[10px] backdrop-blur">{{ tag }}</span>
      </div>
    </div>
    <button
      type="button"
      class="absolute left-2 top-2 z-10 grid size-8 place-items-center rounded-full border-2 text-white shadow-lg backdrop-blur transition active:scale-90"
      :class="[
        selected ? 'border-primary bg-primary opacity-100' : 'border-white/85 bg-black/35',
        !selectionMode && !selected && 'pointer-events-none opacity-0 md:pointer-events-auto md:group-hover:opacity-100',
      ]"
      :aria-label="selected ? '取消选择' : '选择图片'"
      @click.stop="emit('select', index, $event)"
    >
      <Icon :name="selected ? 'tabler:check' : 'tabler:plus'" class="size-4" />
    </button>
  </div>
</template>
