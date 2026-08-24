import type { GalleryPhoto } from '~~/shared/types/photo'

export function useViewerState() {
  const currentPhotoIndex = useState('viewer-index', () => 0)
  const isViewerOpen = useState('viewer-open', () => false)
  const returnRoute = useState<string | null>('viewer-return-route', () => null)
  const returnScrollY = useState('viewer-return-scroll-y', () => 0)
  const scopedPhotos = useState<GalleryPhoto[] | null>('viewer-scope', () => null)

  const openViewer = (index: number, route: string | null = null, photos: GalleryPhoto[] | null = null) => {
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
    scopedPhotos.value = null
  }

  return { currentPhotoIndex, isViewerOpen, returnRoute, returnScrollY, scopedPhotos, openViewer, switchToIndex, closeViewer, clearReturnRoute, clearViewerContext }
}
