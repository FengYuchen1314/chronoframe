<script setup lang="ts">
import type { GalleryPhoto } from '~~/shared/types/photo'

const props = defineProps<{ photo: GalleryPhoto; index: number }>()
const emit = defineEmits<{ openViewer: [number] }>()
const loaded = ref(false)
const source = ref(props.photo.thumbnailUrl)

watch(() => props.photo.thumbnailUrl, value => {
  source.value = value
  loaded.value = false
})

const aspectRatio = computed(() => props.photo.aspectRatio || 1.2)
const camera = computed(() => [props.photo.exif?.Make, props.photo.exif?.Model].filter(Boolean).join(' '))
const handleError = () => {
  if (source.value !== props.photo.originalUrl) source.value = props.photo.originalUrl
}
</script>

<template>
  <button
    type="button"
    class="group relative block w-full touch-manipulation cursor-zoom-in overflow-hidden bg-neutral-200 text-left transition-opacity active:opacity-80 dark:bg-neutral-800"
    :style="{ aspectRatio }"
    :aria-label="photo.title || $t('ui.photo.altFallback')"
    @click="emit('openViewer', index)"
  >
    <div v-if="!loaded" class="absolute inset-0 animate-pulse bg-neutral-200 dark:bg-neutral-800" />
    <img
      :src="source"
      :alt="photo.title"
      loading="lazy"
      decoding="async"
      class="absolute inset-0 h-full w-full object-cover transition duration-500 group-hover:scale-[1.035]"
      :class="loaded ? 'opacity-100' : 'opacity-0'"
      @load="loaded = true"
      @error="handleError"
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
  </button>
</template>
