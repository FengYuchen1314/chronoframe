<script setup lang="ts">
import type { RustPhoto } from '~~/shared/types/photo'
import type { ViewerReturnTarget } from '~/composables/useViewerState'

const router = useRouter()
const route = useRoute()
const colorMode = useColorMode()
const { settings: siteSettings, loaded: siteSettingsLoaded, ensureSiteSettings } = useSiteSettings()

if (!colorMode.preference) colorMode.preference = 'system'
if (import.meta.client) void ensureSiteSettings().catch(() => undefined)

watch(
  () => siteSettingsLoaded.value ? siteSettings.value.theme : null,
  (theme) => {
    if (theme) colorMode.preference = theme
  },
  { immediate: true },
)

const { data, refresh: refreshPhotos, status } = useFetch<RustPhoto[]>('/api/photos', {
  server: false,
  default: () => [],
})

const photos = computed(() => (data.value || []).map(adaptRustPhoto))
const refresh = async () => { await refreshPhotos() }

const viewer = useViewerState()
const viewerPhotos = computed(() => viewer.scopedPhotos.value ?? photos.value)
const viewerClosing = ref(false)

const nextAnimationFrame = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()))

const containedImageRect = (image: HTMLImageElement) => {
  const bounds = image.getBoundingClientRect()
  if (!image.naturalWidth || !image.naturalHeight) return bounds
  const ratio = Math.min(bounds.width / image.naturalWidth, bounds.height / image.naturalHeight)
  const width = image.naturalWidth * ratio
  const height = image.naturalHeight * ratio
  return new DOMRect(
    bounds.left + (bounds.width - width) / 2,
    bounds.top + (bounds.height - height) / 2,
    width,
    height,
  )
}

const createPhotoReturnAnimation = (photoId: string, targetSnapshot: ViewerReturnTarget | null) => {
  if (!import.meta.client || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return null
  const sourceRoot = Array.from(document.querySelectorAll<HTMLElement>('[data-viewer-current="true"]'))
    .find((element) => {
      const rect = element.getBoundingClientRect()
      return getComputedStyle(element).display !== 'none' && rect.width > 0 && rect.height > 0
    })
  const sourceImages = sourceRoot
    ? Array.from(sourceRoot.querySelectorAll<HTMLImageElement>('img')).filter((image) => {
        const rect = image.getBoundingClientRect()
        return getComputedStyle(image).opacity !== '0' && rect.width > 0 && rect.height > 0
      })
    : []
  const source = sourceImages.find(image => image.hasAttribute('data-progressive-full'))
    || sourceImages.find(image => image.hasAttribute('data-progressive-placeholder'))
    || null
  if (!source) return null

  const sourceRect = containedImageRect(source)
  const clone = document.createElement('img')
  clone.src = source.currentSrc || source.src
  clone.alt = ''
  clone.setAttribute('aria-hidden', 'true')
  Object.assign(clone.style, {
    position: 'fixed',
    zIndex: '110',
    pointerEvents: 'none',
    objectFit: 'cover',
    top: `${sourceRect.top}px`,
    left: `${sourceRect.left}px`,
    width: `${sourceRect.width}px`,
    height: `${sourceRect.height}px`,
    borderRadius: '0px',
    boxShadow: '0 18px 50px rgb(0 0 0 / 0.25)',
    willChange: 'top,left,width,height,border-radius,opacity',
  })
  document.body.appendChild(clone)
  const cleanup = () => {
    clone.remove()
  }
  const play = async () => {
    if (!targetSnapshot) {
      await clone.animate(
        [{ opacity: 1, transform: 'scale(1)' }, { opacity: 0, transform: 'scale(.94)' }],
        { duration: 180, easing: 'ease-out', fill: 'forwards' },
      ).finished.catch(() => undefined)
      return
    }
    await clone.animate([
      {
        top: `${sourceRect.top}px`,
        left: `${sourceRect.left}px`,
        width: `${sourceRect.width}px`,
        height: `${sourceRect.height}px`,
        borderRadius: '0px',
        opacity: 1,
      },
      {
        top: `${targetSnapshot.top}px`,
        left: `${targetSnapshot.left}px`,
        width: `${targetSnapshot.width}px`,
        height: `${targetSnapshot.height}px`,
        borderRadius: targetSnapshot.borderRadius,
        opacity: 1,
      },
    ], {
      duration: 360,
      easing: 'cubic-bezier(.2,.82,.2,1)',
      fill: 'forwards',
    }).finished.catch(() => undefined)
  }
  const reconcile = async () => {
    if (!targetSnapshot) return
    let target: HTMLElement | null = null
    for (let attempt = 0; attempt < 30; attempt++) {
      target = document.querySelector<HTMLElement>(`[data-photo-id="${CSS.escape(photoId)}"]`)
      if (target && target.getBoundingClientRect().width > 0) break
      await new Promise(resolve => setTimeout(resolve, 40))
    }
    if (!target) {
      await clone.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 140, fill: 'forwards' }).finished.catch(() => undefined)
      return
    }

    const actualRect = target.getBoundingClientRect()
    const visual = target.querySelector<HTMLElement>('.progressive-image') || target
    const actualRadius = getComputedStyle(visual).borderRadius || targetSnapshot.borderRadius
    const previousVisibility = target.style.visibility
    target.style.visibility = 'hidden'
    try {
      await clone.animate([
        {
          top: `${targetSnapshot.top}px`,
          left: `${targetSnapshot.left}px`,
          width: `${targetSnapshot.width}px`,
          height: `${targetSnapshot.height}px`,
          borderRadius: targetSnapshot.borderRadius,
          opacity: 1,
        },
        {
          top: `${actualRect.top}px`,
          left: `${actualRect.left}px`,
          width: `${actualRect.width}px`,
          height: `${actualRect.height}px`,
          borderRadius: actualRadius,
          opacity: 1,
        },
      ], { duration: 120, easing: 'ease-out', fill: 'forwards' }).finished.catch(() => undefined)
    } finally {
      target.style.visibility = previousVisibility
    }
    await clone.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 100, fill: 'forwards' }).finished.catch(() => undefined)
  }
  return { cleanup, play, reconcile }
}

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
  const returnScrollY = viewer.returnScrollY.value
  const currentPhoto = viewerPhotos.value[viewer.currentPhotoIndex.value]
  const targetSnapshot = currentPhoto ? viewer.returnTargets.value[currentPhoto.id] || null : null
  const returnAnimation = currentPhoto ? createPhotoReturnAnimation(currentPhoto.id, targetSnapshot) : null
  try {
    viewer.closeViewer()
    const navigation = (async () => {
      await router.replace(destination)
      await nextTick()
      if (import.meta.client) window.scrollTo({ top: returnScrollY, behavior: 'auto' })
      if (import.meta.client) await nextAnimationFrame()
    })()
    await Promise.all([navigation, returnAnimation?.play()])
    await returnAnimation?.reconcile()
  } finally {
    returnAnimation?.cleanup()
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
  titleTemplate: title => `${title ? `${title} | ` : ''}${siteSettings.value.title}`,
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
