<script setup lang="ts">
import type { RustPhoto } from '~~/shared/types/photo'

// Keep existing shared links working, without building a whole-site gallery.
definePageMeta({ validate: route => route.params.photoId !== 'dashboard' })
const route = useRoute()
const router = useRouter()
const photoId = computed(() => String(route.params.photoId || ''))
const { data, error } = useFetch<RustPhoto>(() => `/api/photos/${encodeURIComponent(photoId.value)}`, { server: false })
watch(data, photo => {
  if (!photo?.albumId || photo.id !== photoId.value) return
  void router.replace({ path: `/albums/${photo.albumId}`, query: { photo: photo.id } })
}, { immediate: true })
</script>

<template>
  <div class="grid min-h-svh place-items-center">
    <div v-if="error" class="text-center"><p>图片不存在或暂时无法加载</p><UButton to="/albums" class="mt-4">返回相册</UButton></div>
    <Icon v-else name="tabler:loader-2" class="size-8 animate-spin text-primary" />
  </div>
</template>
