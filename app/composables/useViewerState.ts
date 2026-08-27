import type { GalleryPhoto } from '~~/shared/types/photo'

export function useViewerState() {
  const currentPhotoIndex = useState('viewer-index', () => 0)
  const isViewerOpen = useState('viewer-open', () => false)
  const isViewerClosing = useState('viewer-closing', () => false)
  const returnRoute = useState<string | null>('viewer-return-route', () => null)
  const scopedPhotos = useState<GalleryPhoto[] | null>('viewer-scope', () => null)

  const openViewer = (index: number, route: string, photos: GalleryPhoto[]) => {
    if (isViewerClosing.value) return
    // The gallery stays mounted; measure only the actual target when closing.
    currentPhotoIndex.value = index
    returnRoute.value = route
    scopedPhotos.value = photos
    isViewerOpen.value = true
  }
  const switchToIndex = (index: number) => { currentPhotoIndex.value = index }
  const closeViewer = () => { isViewerOpen.value = false }
  const clearViewerContext = () => {
    returnRoute.value = null
    scopedPhotos.value = null
  }

  return { currentPhotoIndex, isViewerOpen, isViewerClosing, returnRoute, scopedPhotos, openViewer, switchToIndex, closeViewer, clearViewerContext }
}
