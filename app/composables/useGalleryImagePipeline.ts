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
  let backgroundPaused = false
  let backgroundQueue: GalleryPhoto[] = []
  let activeBackgroundCount = 0
  const maxBackgroundConcurrency = 2

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
    backgroundQueue = []
    activeBackgroundCount = 0
  }

  const pumpBackgroundQueue = () => {
    if (backgroundPaused || !import.meta.client) return
    while (activeBackgroundCount < maxBackgroundConcurrency && backgroundQueue.length) {
      const photo = backgroundQueue.shift()
      if (!photo || readyOriginalIds.value.has(photo.id) || activePreloads.has(photo.id)) continue
      activeBackgroundCount += 1
      preloadOriginal(photo, 'low')
    }
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
      const entry = activePreloads.get(photo.id)
      activePreloads.delete(photo.id)
      if (entry?.priority === 'low') activeBackgroundCount = Math.max(0, activeBackgroundCount - 1)
      if (requestGeneration !== generation) return
      replaceSetWith(readyOriginalIds, photo.id)
      pumpBackgroundQueue()
    }
    image.onerror = () => {
      const entry = activePreloads.get(photo.id)
      activePreloads.delete(photo.id)
      if (entry?.priority === 'low') activeBackgroundCount = Math.max(0, activeBackgroundCount - 1)
      pumpBackgroundQueue()
    }
    activePreloads.set(photo.id, { image, priority })
    image.src = photo.originalUrl
  }

  const startBackgroundOriginals = () => {
    if (backgroundStarted || !import.meta.client || !photos.value.length) return
    backgroundStarted = true
    backgroundPaused = false
    backgroundQueue = [...photos.value]
    pumpBackgroundQueue()
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

  const pauseBackgroundOriginals = () => {
    backgroundPaused = true
    backgroundQueue = []
    for (const [photoId, entry] of activePreloads) {
      if (entry.priority !== 'low') continue
      stopPreload(entry)
      activePreloads.delete(photoId)
    }
    activeBackgroundCount = 0
  }

  const isOriginalReady = (photoId: string) => readyOriginalIds.value.has(photoId)

  watch(
    () => photos.value.map(photo => photo.id).join('\u0000'),
    () => {
      generation += 1
      backgroundStarted = false
      backgroundPaused = false
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

  return { isOriginalReady, markThumbnailSettled, prioritizeAround, pauseBackgroundOriginals }
}
