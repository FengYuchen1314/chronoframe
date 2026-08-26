import type {
  GalleryAlbum,
  GalleryPhoto,
  RustAlbum,
  RustAlbumDetailPayload,
  RustPhoto,
} from '~~/shared/types/photo'

const asIsoDate = (value?: number | string | null) => {
  if (typeof value === 'number') {
    const milliseconds = value < 10_000_000_000 ? value * 1000 : value
    return new Date(milliseconds).toISOString()
  }
  if (typeof value === 'string' && value) {
    const numeric = Number(value)
    if (Number.isFinite(numeric)) return asIsoDate(numeric)
    const parsed = new Date(value)
    if (!Number.isNaN(parsed.getTime())) return parsed.toISOString()
  }
  return ''
}

export const adaptRustPhoto = (photo: RustPhoto): GalleryPhoto => {
  const width = photo.width && photo.width > 0 ? photo.width : null
  const height = photo.height && photo.height > 0 ? photo.height : null
  const dateTaken = asIsoDate(photo.dateTaken ?? photo.createdAt)
  return {
    id: String(photo.id),
    albumId: photo.albumId ? String(photo.albumId) : null,
    title: photo.originalName?.trim() || '',
    description: photo.description?.trim() || '',
    format: photo.format?.toLowerCase() || '',
    fileSize: Number(photo.byteSize) || 0,
    createdAt: asIsoDate(photo.createdAt),
    dateTaken,
    width,
    height,
    aspectRatio: width && height ? width / height : null,
    originalUrl: `/api/photos/${encodeURIComponent(photo.id)}/file`,
    thumbnailUrl: `/api/photos/${encodeURIComponent(photo.id)}/thumbnail?v=png1`,
    storageKey: photo.storageKey?.trim() || '',
    tags: Array.isArray(photo.tags) ? photo.tags.filter(Boolean) : [],
    exif: photo.exif ?? null,
    city: photo.city?.trim() || null,
    country: photo.country?.trim() || null,
    locationName: photo.locationName?.trim() || null,
  }
}

export const adaptRustAlbum = (album: RustAlbum, allPhotos: GalleryPhoto[] = []): GalleryAlbum => {
  const embeddedPhotos = Array.isArray(album.photos) ? album.photos.map(adaptRustPhoto) : []
  const photos = embeddedPhotos.length
    ? embeddedPhotos
    : allPhotos.filter(photo => photo.albumId === String(album.id))
  return {
    id: String(album.id),
    title: album.name?.trim() || album.title?.trim() || `Album ${album.id}`,
    description: album.description?.trim() || '',
    createdAt: asIsoDate(album.createdAt),
    displayCreatedDate: album.displayCreatedDate?.trim() || null,
    photoDateStart: album.photoDateStart?.trim() || null,
    photoDateEnd: album.photoDateEnd?.trim() || null,
    position: Number(album.position) || 0,
    photoCount: Number(album.photoCount) || photos.length,
    photos,
    photoIds: photos.map(photo => photo.id),
    coverPhotoId: photos[0]?.id ?? null,
  }
}

export const adaptRustAlbumDetail = (
  payload: RustAlbumDetailPayload,
  fallbackPhotos: GalleryPhoto[] = [],
): GalleryAlbum => {
  const album = payload.album ?? payload as RustAlbum
  const photos = payload.photos ?? album.photos ?? []
  return adaptRustAlbum({ ...album, photos }, fallbackPhotos)
}

export const formatGalleryDate = (value: string, options: Intl.DateTimeFormatOptions = { dateStyle: 'medium' }) => {
  const parsed = new Date(value)
  if (Number.isNaN(parsed.getTime()) || parsed.getTime() === 0) return '—'
  return new Intl.DateTimeFormat(undefined, options).format(parsed)
}

export const formatGalleryCalendarDate = (value: string, options: Intl.DateTimeFormatOptions = { dateStyle: 'medium' }) => {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value)
  if (!match) return '—'

  const date = new Date(0)
  date.setUTCHours(0, 0, 0, 0)
  date.setUTCFullYear(Number(match[1]), Number(match[2]) - 1, Number(match[3]))
  if (
    date.getUTCFullYear() !== Number(match[1])
    || date.getUTCMonth() !== Number(match[2]) - 1
    || date.getUTCDate() !== Number(match[3])
  ) return '—'

  return new Intl.DateTimeFormat(undefined, { ...options, timeZone: 'UTC' }).format(date)
}

export const formatBytes = (size: number) => {
  if (!size) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const unit = Math.min(Math.floor(Math.log(size) / Math.log(1024)), units.length - 1)
  return `${(size / 1024 ** unit).toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`
}
