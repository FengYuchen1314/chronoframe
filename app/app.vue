<script setup lang="ts">
import type { RustPhoto } from '~~/shared/types/photo'

const router = useRouter()
const route = useRoute()
const colorMode = useColorMode()
const { settings: siteSettings, loaded: siteSettingsLoaded, ensureSiteSettings } = useSiteSettings()

if (!colorMode.preference) colorMode.preference = 'system'
if (import.meta.client) void ensureSiteSettings().catch(() => undefined)
watch(
  () => siteSettingsLoaded.value ? siteSettings.value.theme : null,
  theme => { if (theme) colorMode.preference = theme },
  { immediate: true },
)

// An album detail already carries its own photos. Do not fetch the whole site
// merely to open an album or a single-photo deep link.
const { data, refresh: refreshPhotos, status } = useFetch<RustPhoto[]>('/api/photos', {
  server: false, immediate: false, watch: false, default: () => [],
})
const photos = computed(() => (data.value || []).map(adaptRustPhoto))
const refresh = async () => { await refreshPhotos() }
const needsCatalogue = computed(() => ['/', '/albums', '/photos'].includes(route.path))
watch(needsCatalogue, needed => { if (needed && status.value === 'idle') void refreshPhotos() }, { immediate: true })

const viewer = useViewerState()
const viewerPhotos = computed(() => viewer.scopedPhotos.value ?? photos.value)
const viewerClosing = viewer.isViewerClosing
let returnAnimation: ReturnType<typeof createViewerReturnAnimation> = null

const handleIndexChange = (index: number) => {
  if (viewerClosing.value) return
  const photo = viewerPhotos.value[index]
  if (!photo) return
  viewer.switchToIndex(index)
  void router.replace({ path: route.path, query: { ...route.query, photo: photo.id }, hash: route.hash })
}

const handleClose = async (updateRoute = true) => {
  if (viewerClosing.value || !viewer.isViewerOpen.value) return
  viewerClosing.value = true
  const photo = viewerPhotos.value[viewer.currentPhotoIndex.value]
  try {
    returnAnimation = photo ? createViewerReturnAnimation(photo.id) : null
    // The background gallery is already ready. Only the overlay fades while a
    // clone returns in one transform; no network, remount, or second correction.
    if (returnAnimation) await returnAnimation.finished
    else if (!document.hidden && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      await new Promise(resolve => setTimeout(resolve, 160))
    }
    if (updateRoute && route.query.photo === photo?.id) {
      const { photo: _photo, ...query } = route.query
      await router.replace({ path: route.path, query, hash: route.hash })
    }
  } finally {
    returnAnimation?.cleanup()
    returnAnimation = null
    viewer.closeViewer()
    viewer.clearViewerContext()
    viewerClosing.value = false
    await nextTick()
    if (photo) document.querySelector<HTMLElement>(`[data-photo-id="${CSS.escape(photo.id)}"] [role="button"]`)?.focus({ preventScroll: true })
  }
}

watch(() => [route.path, route.query.photo] as const, ([path, photo], previous) => {
  if (!previous || !viewer.isViewerOpen.value) return
  if (path !== previous[0]) {
    returnAnimation?.cleanup()
    viewer.closeViewer()
    viewer.clearViewerContext()
  } else if (!photo && previous[1] && !viewerClosing.value) {
    // Browser back closes the same overlay without destroying the album.
    void handleClose(false)
  }
})
watch(() => route.path, (path, previousPath) => {
  if (needsCatalogue.value && previousPath?.startsWith('/dashboard')) void refreshPhotos()
})
onBeforeUnmount(() => returnAnimation?.cleanup())
useHead({ titleTemplate: title => `${title ? `${title} | ` : ''}${siteSettings.value.title}` })
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
          :closing="viewerClosing"
          @close="handleClose()"
          @index-change="handleIndexChange"
        />
      </ClientOnly>
    </PhotosProvider>
  </UApp>
</template>
