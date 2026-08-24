import type { GalleryPhoto } from '~~/shared/types/photo'

export interface ViewerReturnTarget {
  left: number
  top: number
  width: number
  height: number
  borderRadius: string
}

export function useViewerState() {
  const currentPhotoIndex = useState('viewer-index', () => 0)
  const isViewerOpen = useState('viewer-open', () => false)
  const returnRoute = useState<string | null>('viewer-return-route', () => null)
  const returnScrollY = useState('viewer-return-scroll-y', () => 0)
  const returnTargets = useState<Record<string, ViewerReturnTarget>>('viewer-return-targets', () => ({}))
  const scopedPhotos = useState<GalleryPhoto[] | null>('viewer-scope', () => null)

  const openViewer = (index: number, route: string | null = null, photos: GalleryPhoto[] | null = null) => {
    if (import.meta.client) {
      const allowedIds = new Set((photos || []).map(photo => photo.id))
      const targets: Record<string, ViewerReturnTarget> = {}
      for (const element of document.querySelectorAll<HTMLElement>('[data-photo-id]')) {
        const photoId = element.dataset.photoId
        if (!photoId || (allowedIds.size && !allowedIds.has(photoId))) continue
        const rect = element.getBoundingClientRect()
        if (rect.width <= 0 || rect.height <= 0) continue
        const visual = element.querySelector<HTMLElement>('.progressive-image') || element
        targets[photoId] = {
          left: rect.left,
          top: rect.top,
          width: rect.width,
          height: rect.height,
          borderRadius: getComputedStyle(visual).borderRadius || '0px',
        }
      }
      returnTargets.value = targets
    }
    currentPhotoIndex.value = index
    returnRoute.value = route
    returnScrollY.value = import.meta.client ? window.scrollY : 0
    scopedPhotos.value = photos
    isViewerOpen.value = true
  }
  const switchToIndex = (index: number) => { currentPhotoIndex.value = index }
  const closeViewer = () => { isViewerOpen.value = false }
  const clearReturnRoute = () => { returnRoute.value = null }
  const clearViewerContext = () => {
    returnRoute.value = null
    returnScrollY.value = 0
    returnTargets.value = {}
    scopedPhotos.value = null
  }

  return { currentPhotoIndex, isViewerOpen, returnRoute, returnScrollY, returnTargets, scopedPhotos, openViewer, switchToIndex, closeViewer, clearReturnRoute, clearViewerContext }
}
