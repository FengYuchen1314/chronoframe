import { viewerNeighborIndices } from './viewerPerformance.ts'

interface PreviewPhoto { id: string; previewUrl: string }
interface PreviewImage {
  src: string
  decoding: string
  fetchPriority: string
  onload: unknown
  onerror: unknown
  removeAttribute(name: string): void
}

/** A bounded, moving preview window. Never restart an in-flight new current photo. */
export function createViewerPreloader(onReady: (ids: Set<string>) => void, createImage: () => PreviewImage) {
  const requests = new Map<string, { image: PreviewImage; done: boolean }>()
  let windowPhotos: PreviewPhoto[] = []
  let current: PreviewPhoto | undefined
  let readyUrls = new Set<string>()
  let currentReady = false
  const notify = () => onReady(new Set(windowPhotos.filter(photo => readyUrls.has(photo.previewUrl)).map(photo => photo.id)))
  const discard = (url: string) => {
    const entry = requests.get(url)
    if (!entry) return
    requests.delete(url)
    entry.image.onload = null
    entry.image.onerror = null
    if (!entry.done) entry.image.removeAttribute('src')
  }
  const pump = () => {
    if (!currentReady) return
    for (const photo of windowPhotos) {
      if (photo === current || readyUrls.has(photo.previewUrl) || requests.has(photo.previewUrl)) continue
      if ([...requests.values()].filter(entry => !entry.done).length >= 2) break
      const image = createImage()
      image.decoding = 'async'
      image.fetchPriority = 'low'
      const entry = { image, done: false }
      requests.set(photo.previewUrl, entry)
      const finish = (ready: boolean) => {
        // A cancelled request can still dispatch its old completion callback.
        if (requests.get(photo.previewUrl) !== entry) return
        entry.done = true
        image.onload = null
        image.onerror = null
        if (ready) readyUrls.add(photo.previewUrl)
        if (ready && current?.previewUrl === photo.previewUrl) currentReady = true
        notify()
        pump()
      }
      image.onload = () => finish(true)
      image.onerror = () => finish(false)
      image.src = photo.previewUrl
    }
  }
  return {
    setWindow(photos: PreviewPhoto[], index: number) {
      current = photos[index]
      windowPhotos = current ? [current, ...viewerNeighborIndices(index, photos.length).map(i => photos[i]!)] : []
      const wanted = new Set(windowPhotos.map(photo => photo.previewUrl))
      for (const [url, entry] of requests) {
        if (!wanted.has(url)) discard(url)
        else entry.image.fetchPriority = url === current?.previewUrl ? 'high' : 'low'
      }
      readyUrls = new Set([...readyUrls].filter(url => wanted.has(url)))
      currentReady = !!current && readyUrls.has(current.previewUrl)
      notify()
      pump()
    },
    markReady(photoId: string) {
      if (current?.id !== photoId) return
      readyUrls.add(current.previewUrl)
      currentReady = true
      notify()
      pump()
    },
    clear() {
      current = undefined
      windowPhotos = []
      currentReady = false
      for (const url of requests.keys()) discard(url)
      readyUrls.clear()
      notify()
    },
  }
}
