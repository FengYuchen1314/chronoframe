import type { GalleryPhoto } from '~~/shared/types/photo'

/** A photo is a query-backed overlay, never a replacement for its gallery. */
export function useViewerRoute(photos: Ref<GalleryPhoto[]>, galleryPath: MaybeRefOrGetter<string>) {
  const route = useRoute()
  const router = useRouter()
  const viewer = useViewerState()
  const galleryLocation = () => {
    const { photo: _photo, ...query } = route.query
    return { path: toValue(galleryPath), query, hash: route.hash }
  }

  const openPhoto = (index: number) => {
    const photo = photos.value[index]
    if (!photo || viewer.isViewerClosing.value) return
    const location = galleryLocation()
    viewer.openViewer(index, router.resolve(location).fullPath, photos.value)
    void router.push({ ...location, query: { ...location.query, photo: photo.id } })
  }

  watch([() => route.query.photo, photos, viewer.isViewerClosing], ([id, available, closing]) => {
    if (closing || route.path !== toValue(galleryPath) || typeof id !== 'string') return
    const index = available.findIndex(photo => photo.id === id)
    if (index < 0) return
    if (viewer.isViewerOpen.value) {
      viewer.scopedPhotos.value = available
      viewer.switchToIndex(index)
    } else {
      viewer.openViewer(index, router.resolve(galleryLocation()).fullPath, available)
    }
  }, { immediate: true })

  return { openPhoto }
}
