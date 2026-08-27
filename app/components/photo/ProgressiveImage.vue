<script setup lang="ts">
const props = withDefaults(defineProps<{
  src: string
  placeholderSrc?: string | null
  fallbackSrc?: string | null
  alt?: string
  fit?: 'contain' | 'cover'
  loading?: 'eager' | 'lazy'
  fetchPriority?: 'high' | 'low' | 'auto'
  draggable?: boolean
  pulse?: boolean
}>(), {
  placeholderSrc: null,
  fallbackSrc: null,
  alt: '',
  fit: 'cover',
  loading: 'lazy',
  fetchPriority: 'auto',
  draggable: false,
  pulse: true,
})

const emit = defineEmits<{
  load: [HTMLImageElement]
  error: [Event]
}>()

const resolvedSrc = ref(props.src)
const fullImage = ref<HTMLImageElement>()
const loaded = ref(false)
const failed = ref(false)
const cached = ref(false)
let generation = 0
let notifiedGeneration = -1

watch(() => props.src, (src) => {
  generation += 1
  resolvedSrc.value = src
  loaded.value = false
  failed.value = false
  cached.value = false
  void nextTick(checkCachedImage)
}, { flush: 'sync' })

const reveal = async (image: HTMLImageElement, fromCache = false) => {
  const request = generation
  // Decode only this image. Never wait for a gallery-wide preload barrier.
  try { await image.decode() } catch { /* A valid loaded image can still be drawn. */ }
  if (request !== generation || image !== fullImage.value || !image.naturalWidth) return
  cached.value = fromCache
  loaded.value = true
  failed.value = false
  if (notifiedGeneration !== request) {
    notifiedGeneration = request
    emit('load', image)
  }
}
const onLoad = (event: Event) => { void reveal(event.currentTarget as HTMLImageElement) }
const checkCachedImage = () => {
  const image = fullImage.value
  if (image?.complete && image.naturalWidth) void reveal(image, true)
}
onMounted(checkCachedImage)
onBeforeUnmount(() => { generation += 1 })

const onError = (event: Event) => {
  if (props.fallbackSrc && resolvedSrc.value !== props.fallbackSrc) {
    generation += 1
    resolvedSrc.value = props.fallbackSrc
    loaded.value = false
    void nextTick(checkCachedImage)
    return
  }
  failed.value = true
  emit('error', event)
}
</script>

<template>
  <div class="progressive-image relative overflow-hidden" :class="fit === 'cover' ? 'bg-neutral-200 dark:bg-neutral-800' : 'bg-transparent'" :data-image-loaded="loaded">
    <div
      v-if="fit === 'cover'"
      class="absolute inset-0 bg-linear-to-br from-neutral-200 via-neutral-100 to-neutral-300 transition-opacity duration-300 dark:from-neutral-800 dark:via-neutral-700 dark:to-neutral-900"
      :class="[
        pulse && !loaded ? 'progressive-image-pulse' : '',
        loaded ? 'opacity-0' : 'opacity-100',
      ]"
    />
    <img
      v-if="placeholderSrc && placeholderSrc !== resolvedSrc"
      data-progressive-placeholder
      :src="placeholderSrc"
      alt=""
      aria-hidden="true"
      loading="eager"
      decoding="async"
      :draggable="false"
      class="absolute inset-0 h-full w-full transition-opacity duration-100"
      :class="[
        fit === 'contain' ? 'object-contain' : 'object-cover',
        loaded ? 'opacity-0' : 'opacity-100',
      ]"
    />
    <img
      ref="fullImage"
      data-progressive-full
      :src="resolvedSrc"
      :alt="alt"
      :loading="loading"
      :fetchpriority="fetchPriority"
      decoding="async"
      :draggable="draggable"
      class="absolute inset-0 h-full w-full"
      :class="[
        !cached && 'transition-opacity duration-100',
        fit === 'contain' ? 'object-contain' : 'object-cover',
        loaded ? 'opacity-100' : 'opacity-0',
      ]"
      @load="onLoad"
      @error="onError"
    />
    <div v-if="failed && !placeholderSrc" class="absolute inset-0 grid place-items-center text-neutral-400">
      <Icon name="tabler:photo-off" class="size-7" />
    </div>
  </div>
</template>

<style scoped>
@keyframes progressive-image-pulse {
  0%, 100% { opacity: 0.58; }
  50% { opacity: 1; }
}
.progressive-image-pulse { animation: progressive-image-pulse 1.45s ease-in-out infinite; }
@media (prefers-reduced-motion: reduce) {
  .progressive-image-pulse { animation: none; }
}
</style>
