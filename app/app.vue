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

const createPhotoReturnAnimation = (photoId: string) => {
  if (!import.meta.client || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return null
  const source = Array.from(document.querySelectorAll<HTMLImageElement>('[data-viewer-current="true"]'))
    .find((image) => {
      const rect = image.getBoundingClientRect()
      return getComputedStyle(image).display !== 'none' && rect.width > 0 && rect.height > 0
    })
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
  const previousSourceVisibility = source.style.visibility
  source.style.visibility = 'hidden'

  const cleanup = () => {
    source.style.visibility = previousSourceVisibility
    clone.remove()
  }
  const play = async () => {
    let target: HTMLElement | null = null
    for (let attempt = 0; attempt < 24; attempt++) {
      target = document.querySelector<HTMLElement>(`[data-photo-id="${CSS.escape(photoId)}"]`)
      if (target && target.getBoundingClientRect().width > 0) break
      await new Promise(resolve => setTimeout(resolve, 50))
    }
    if (!target) {
      await clone.animate(
        [{ opacity: 1, transform: 'scale(1)' }, { opacity: 0, transform: 'scale(.94)' }],
        { duration: 180, easing: 'ease-out', fill: 'forwards' },
      ).finished.catch(() => undefined)
      return
    }

    const targetRect = target.getBoundingClientRect()
    const targetImage = target.querySelector<HTMLElement>('img')
    const targetRadius = targetImage ? getComputedStyle(targetImage).borderRadius : '0.75rem'
    const previousTargetVisibility = target.style.visibility
    target.style.visibility = 'hidden'
    try {
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
          top: `${targetRect.top}px`,
          left: `${targetRect.left}px`,
          width: `${targetRect.width}px`,
          height: `${targetRect.height}px`,
          borderRadius: targetRadius,
          opacity: 1,
        },
      ], {
        duration: 380,
        easing: 'cubic-bezier(.2,.8,.2,1)',
        fill: 'forwards',
      }).finished.catch(() => undefined)
    } finally {
      target.style.visibility = previousTargetVisibility
    }
  }
  return { cleanup, play }
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
  const returnAnimation = currentPhoto ? createPhotoReturnAnimation(currentPhoto.id) : null
  try {
    // Keep the scoped photo list alive until the photo route has gone away.
    // Clearing it first lets [photoId].vue observe a different list and reopen
    // the viewer, which made the close button appear to require two clicks.
    await router.replace(destination)
    await nextTick()
    if (import.meta.client) window.scrollTo({ top: returnScrollY, behavior: 'auto' })
    if (import.meta.client) await nextAnimationFrame()
    viewer.closeViewer()
    viewer.clearViewerContext()
    await nextTick()
    await returnAnimation?.play()
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
