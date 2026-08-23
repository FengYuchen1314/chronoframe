<script setup lang="ts">
import type { AsyncDataRequestStatus } from '#app'
import type { GalleryPhoto } from '~~/shared/types/photo'

const props = defineProps<{
  photos: GalleryPhoto[]
  refresh: () => Promise<void>
  status: AsyncDataRequestStatus
}>()

providePhotos(toRef(props, 'photos'), toRef(props, 'status'), props.refresh)
</script>

<template><slot /></template>
