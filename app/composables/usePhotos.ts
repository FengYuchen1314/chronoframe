import type { AsyncDataRequestStatus } from '#app'
import type { GalleryPhoto } from '~~/shared/types/photo'

interface PhotosContext {
  photos: Ref<GalleryPhoto[]>
  status: Ref<AsyncDataRequestStatus>
  refresh: () => Promise<void>
  getPhotoById: (id: string) => GalleryPhoto | undefined
  totalCount: ComputedRef<number>
}

const PhotosContextKey = Symbol('PhotosContext') as InjectionKey<PhotosContext>

export function providePhotos(
  photos: Ref<GalleryPhoto[]>,
  status: Ref<AsyncDataRequestStatus>,
  refresh: () => Promise<void>,
) {
  const context: PhotosContext = {
    photos,
    status,
    refresh,
    getPhotoById: id => photos.value.find(photo => photo.id === id),
    totalCount: computed(() => photos.value.length),
  }
  provide(PhotosContextKey, context)
  return context
}

export function usePhotos() {
  const context = inject(PhotosContextKey)
  if (!context) throw new Error('usePhotos must be used within PhotosProvider')
  return context
}
