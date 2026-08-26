export interface GalleryExif {
  Make?: string | null
  Model?: string | null
  LensMake?: string | null
  LensModel?: string | null
  Rating?: number | null
  FNumber?: number | null
  ExposureTime?: string | number | null
  ISO?: number | null
  ISOSpeedRatings?: number | null
  FocalLength?: string | number | null
  ImageDescription?: string | null
}

export interface RustPhoto {
  id: string
  albumId?: string | null
  originalName?: string | null
  storageKey?: string | null
  contentType?: string | null
  format?: string | null
  byteSize?: number | null
  createdAt?: number | string | null
  width?: number | null
  height?: number | null
  tags?: string[] | null
  exif?: GalleryExif | null
  city?: string | null
  country?: string | null
  locationName?: string | null
  dateTaken?: number | string | null
  description?: string | null
}

export interface GalleryPhoto {
  id: string
  albumId: string | null
  title: string
  description: string
  format: string
  fileSize: number
  createdAt: string
  dateTaken: string
  width: number | null
  height: number | null
  aspectRatio: number | null
  thumbnailUrl: string
  previewUrl: string
  highUrl: string
  renderUrl: string
  storageKey: string
  tags: string[]
  exif: GalleryExif | null
  city: string | null
  country: string | null
  locationName: string | null
}

export interface RustAlbum {
  id: string
  name?: string | null
  title?: string | null
  description?: string | null
  createdAt?: number | string | null
  displayCreatedDate?: string | null
  photoDateStart?: string | null
  photoDateEnd?: string | null
  position?: number | null
  photoCount?: number | null
  photos?: RustPhoto[] | null
}

export interface RustAlbumDetailPayload {
  album?: RustAlbum
  photos?: RustPhoto[]
  id?: string
  name?: string | null
  title?: string | null
  description?: string | null
  createdAt?: number | string | null
  displayCreatedDate?: string | null
  photoDateStart?: string | null
  photoDateEnd?: string | null
  position?: number | null
  photoCount?: number | null
}

export interface GalleryAlbum {
  id: string
  title: string
  description: string
  createdAt: string
  displayCreatedDate: string | null
  photoDateStart: string | null
  photoDateEnd: string | null
  position: number
  photoCount: number
  photos: GalleryPhoto[]
  photoIds: string[]
  coverPhotoId: string | null
}
