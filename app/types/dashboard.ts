export type StorageBackend = 'local' | 'webdav' | 's3'

export type ImageTargetFormat = 'png' | 'jpg' | 'jpeg' | 'webp'

export interface Album {
  id: string
  name: string
  createdAt: number
  displayCreatedDate: string | null
  photoDateStart: string | null
  photoDateEnd: string | null
  photoCount: number
}

export interface Photo {
  id: string
  albumId: string
  originalName: string
  storageKey: string
  format: 'png' | 'jpg' | 'webp'
  contentType: string
  byteSize: number
  width: number
  height: number
  createdAt: number
}

export interface AlbumDetail extends Album {
  photos: Photo[]
}

export interface ConversionJob {
  id: string
  status: string
  targetFormat: 'png' | 'jpg' | 'webp'
  total: number
  completed: number
  succeeded: number
  failed: number
  cancelled: number
  createdAt: number
  updatedAt: number
  sourcesDeletedAt: number | null
}

export interface ConversionItem {
  id: string
  sourcePhotoId: string
  sourceName: string
  status: string
  targetPhotoId: string | null
  error: string | null
}

export interface ConversionDetail {
  job: ConversionJob
  items: ConversionItem[]
}

export interface SourceDeletionFailure {
  photoId: string
  error: string
}

export interface SourceDeletionResult {
  removed: number
  failures: SourceDeletionFailure[]
}

export interface StorageSettings {
  backend: StorageBackend
  localPath: string
  webdavUrl: string
  webdavUsername: string
  webdavPrefix: string
  webdavPasswordSet: boolean
  s3Endpoint: string
  s3Region: string
  s3Bucket: string
  s3AccessKey: string
  s3SecretKeySet: boolean
  s3Prefix: string
}

export interface StorageSettingsInput {
  backend: StorageBackend
  localPath: string
  webdavUrl: string
  webdavUsername: string
  webdavPassword?: string
  webdavPrefix: string
  s3Endpoint: string
  s3Region: string
  s3Bucket: string
  s3AccessKey: string
  s3SecretKey?: string
  s3Prefix: string
}
