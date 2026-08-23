<script setup lang="ts">
definePageMeta({ layout: 'masonry', key: 'gallery-route' })
const route = useRoute()
const router = useRouter()
const { clearAllFilters, toggleFilter } = usePhotoFilters()

watch(() => route.query.tag, tag => {
  if (typeof tag !== 'string' || !tag) return
  clearAllFilters()
  toggleFilter('tags', tag)
  router.replace('/photos')
}, { immediate: true })
</script>

<template><div /></template>
