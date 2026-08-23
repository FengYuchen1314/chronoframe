<script setup lang="ts">
definePageMeta({
  layout: 'masonry',
  key: 'gallery-route',
  validate: route => route.params.photoId !== 'dashboard',
})

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const { photos, status } = usePhotos()
const viewer = useViewerState()
const photoId = computed(() => String(route.params.photoId || ''))
const activePhotos = computed(() => viewer.isViewerOpen.value && viewer.scopedPhotos.value
  ? viewer.scopedPhotos.value
  : photos.value)
const currentPhoto = computed(() => activePhotos.value.find(photo => photo.id === photoId.value))

useHead({ title: computed(() => currentPhoto.value?.title || t('title.fallback.photo')) })

watch([photoId, activePhotos, status], ([id, active, requestStatus]) => {
  if (!active.length) {
    if (requestStatus === 'success') router.replace('/photos')
    return
  }
  const index = active.findIndex(photo => photo.id === id)
  if (index < 0) {
    if (requestStatus === 'success') router.replace('/photos')
    return
  }
  if (viewer.isViewerOpen.value) viewer.switchToIndex(index)
  else viewer.openViewer(index, null, null)
}, { immediate: true })
</script>

<template><div /></template>
