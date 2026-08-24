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
  load: [Event]
  error: [Event]
}>()

const resolvedSrc = ref(props.src)
const loaded = ref(false)
const failed = ref(false)

watch(() => props.src, (src) => {
  resolvedSrc.value = src
  loaded.value = false
  failed.value = false
})

const onLoad = (event: Event) => {
  loaded.value = true
  failed.value = false
  emit('load', event)
}

const onError = (event: Event) => {
  if (props.fallbackSrc && resolvedSrc.value !== props.fallbackSrc) {
    resolvedSrc.value = props.fallbackSrc
    loaded.value = false
    return
  }
  failed.value = true
  emit('error', event)
}
</script>

<template>
  <div class="progressive-image relative overflow-hidden bg-neutral-200 dark:bg-neutral-800">
    <div
      class="absolute inset-0 bg-linear-to-br from-neutral-200 via-neutral-100 to-neutral-300 transition-opacity duration-300 dark:from-neutral-800 dark:via-neutral-700 dark:to-neutral-900"
      :class="[
        pulse && !loaded ? 'progressive-image-pulse' : '',
        loaded ? 'opacity-0' : 'opacity-100',
      ]"
    />
    <img
      v-if="placeholderSrc"
      data-progressive-placeholder
      :src="placeholderSrc"
      alt=""
      aria-hidden="true"
      loading="eager"
      decoding="async"
      :draggable="false"
      class="absolute inset-0 h-full w-full scale-[1.02] opacity-100 blur-md transition-opacity duration-300"
      :class="[
        fit === 'contain' ? 'object-contain' : 'object-cover',
        loaded ? 'opacity-0' : 'opacity-100',
      ]"
    />
    <img
      data-progressive-full
      :src="resolvedSrc"
      :alt="alt"
      :loading="loading"
      :fetchpriority="fetchPriority"
      decoding="async"
      :draggable="draggable"
      class="absolute inset-0 h-full w-full transition-opacity duration-300"
      :class="[
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
