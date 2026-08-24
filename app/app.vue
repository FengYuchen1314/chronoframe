<script setup lang="ts">
import type { RustPhoto } from '~~/shared/types/photo'

const router = useRouter()
const route = useRoute()
const config = useRuntimeConfig()
const colorMode = useColorMode()

if (!colorMode.preference) colorMode.preference = 'system'

const { data, refresh: refreshPhotos, status } = useFetch<RustPhoto[]>('/api/photos', {
  server: false,
  default: () => [],
})

const photos = computed(() => (data.value || []).map(adaptRustPhoto))
const refresh = async () => { await refreshPhotos() }

const viewer = useViewerState()
const viewerPhotos = computed(() => viewer.scopedPhotos.value ?? photos.value)
const viewerClosing = ref(false)

const handleIndexChange = (index: number) => {
  if (viewerClosing.value) return
  const photo = viewerPhotos.value[index]
  if (!photo) return
  viewer.switchToIndex(index)
  router.replace(`/${photo.id}`)
}

const handleClose = async () => {
  if (viewerClosing.value) return
  viewerClosing.value = true
  const destination = viewer.returnRoute.value || '/photos'
  try {
    // Keep the scoped photo list alive until the photo route has gone away.
    // Clearing it first lets [photoId].vue observe a different list and reopen
    // the viewer, which made the close button appear to require two clicks.
    await router.replace(destination)
  } finally {
    viewer.closeViewer()
    viewer.clearViewerContext()
    viewerClosing.value = false
  }
}

watch(
  () => route.path,
  (path, previousPath) => {
    if (!path.startsWith('/dashboard') && previousPath?.startsWith('/dashboard')) {
      void refreshPhotos()
    }
  },
)

watch(
  () => route.name,
  (name) => {
    if (!String(name || '').startsWith('photoId') && viewer.isViewerOpen.value) {
      viewer.closeViewer()
      viewer.clearViewerContext()
    }
  },
)

useHead({
  titleTemplate: title => `${title ? `${title} | ` : ''}${config.public.app.title}`,
})
</script>

<template>
  <UApp>
    <NuxtLoadingIndicator color="var(--ui-primary)" />
    <PhotosProvider :photos="photos" :refresh="refresh" :status="status">
      <NuxtLayout>
        <NuxtPage />
      </NuxtLayout>

      <ClientOnly>
        <PhotoViewer
          :photos="viewerPhotos"
          :current-index="viewer.currentPhotoIndex.value"
          :is-open="viewer.isViewerOpen.value"
          @close="handleClose"
          @index-change="handleIndexChange"
        />
      </ClientOnly>
    </PhotosProvider>
  </UApp>
</template>
