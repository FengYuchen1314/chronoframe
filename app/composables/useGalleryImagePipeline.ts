import type { Ref } from 'vue'
import type { GalleryPhoto } from '~~/shared/types/photo'

type ImagePriority = 'high' | 'low'

interface ActivePreload {
  image: HTMLImageElement
  priority: ImagePriority
}

export function useGalleryImagePipeline(photos: Readonly<Ref<GalleryPhoto[]>>) {
  const settledThumbnailIds = shallowRef(new Set<string>())
  const readyOriginalIds = shallowRef(new Set<string>())
  const activePreloads = new Map<string, ActivePreload>()
  let generation = 0
  let backgroundStarted = false

  const replaceSetWith = (target: typeof settledThumbnailIds, value: string) => {
    const next = new Set(target.value)
    next.add(value)
    target.value = next
  }

  const stopPreload = (entry: ActivePreload) => {
    entry.image.onload = null
    entry.image.onerror = null
    if (!entry.image.complete) entry.image.src = ''
  }

  const clearPreloads = () => {
    for (const entry of activePreloads.values()) stopPreload(entry)
    activePreloads.clear()
  }

  const preloadOriginal = (photo: GalleryPhoto, priority: ImagePriority) => {
    if (!import.meta.client || readyOriginalIds.value.has(photo.id)) return
    const existing = activePreloads.get(photo.id)
    if (existing?.priority === 'high' || existing?.priority === priority) return
    if (existing) {
      stopPreload(existing)
      activePreloads.delete(photo.id)
    }

    const requestGeneration = generation
    const image = new Image()
    image.decoding = 'async'
    image.fetchPriority = priority
    image.onload = () => {
      activePreloads.delete(photo.id)
      if (requestGeneration !== generation) return
      replaceSetWith(readyOriginalIds, photo.id)
    }
    image.onerror = () => activePreloads.delete(photo.id)
    activePreloads.set(photo.id, { image, priority })
    image.src = photo.originalUrl
  }

  const startBackgroundOriginals = () => {
    if (backgroundStarted || !import.meta.client || !photos.value.length) return
    backgroundStarted = true
    // Deliberately enqueue the whole original set without a JavaScript concurrency cap.
    // The browser/network stack retains transport-level scheduling while tiles sharpen one by one.
    for (const photo of photos.value) preloadOriginal(photo, 'low')
  }

  const markThumbnailSettled = (photoId: string) => {
    if (settledThumbnailIds.value.has(photoId)) return
    replaceSetWith(settledThumbnailIds, photoId)
    if (settledThumbnailIds.value.size >= photos.value.length) startBackgroundOriginals()
  }

  const prioritizeAround = (index: number, radius = 2) => {
    const start = Math.max(0, index - radius)
    const end = Math.min(photos.value.length - 1, index + radius)
    for (let current = start; current <= end; current += 1) {
      const photo = photos.value[current]
      if (photo) preloadOriginal(photo, 'high')
    }
  }

  const isOriginalReady = (photoId: string) => readyOriginalIds.value.has(photoId)

  watch(
    () => photos.value.map(photo => photo.id).join('\u0000'),
    () => {
      generation += 1
      backgroundStarted = false
      clearPreloads()
      settledThumbnailIds.value = new Set()
      readyOriginalIds.value = new Set()
    },
    { immediate: true },
  )

  onScopeDispose(() => {
    generation += 1
    clearPreloads()
  })

  return { isOriginalReady, markThumbnailSettled, prioritizeAround }
}
