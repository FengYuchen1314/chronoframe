// 后台数据类型定义。
// 逐字移植自 Vue 侧的 app/types/dashboard.ts 与 shared/types/downloads.ts，
// 字段名与后端 camelCase 序列化保持一致，不要重命名。

export type StorageBackend = 'local' | 'webdav' | 's3'
export type SiteTheme = 'light' | 'dark' | 'system'
export type ImageFormat = 'png' | 'jpg' | 'webp'
export type DownloadFormat = 'png' | 'jpg' | 'jpeg' | 'webp'

/* ------------------------------- 站点设置 ------------------------------- */

export interface SiteSettings {
  title: string
  slogan: string
  author: string
  avatarUrl: string
  theme: SiteTheme
}

/* --------------------------------- 相册 --------------------------------- */

export interface AlbumCover {
  coverSource: 'auto' | 'photo' | 'upload'
  coverPhotoId: string | null
  coverUrl: string | null
}

export interface Album extends AlbumCover {
  id: string
  name: string
  description: string
  createdAt: number
  displayCreatedDate: string | null
  photoDateStart: string | null
  photoDateEnd: string | null
  position: number
  photoCount: number
}

export interface Photo {
  id: string
  albumId: string
  originalName: string
  storageKey: string
  format: ImageFormat
  contentType: string
  byteSize: number
  width: number
  height: number
  createdAt: number
}

export interface AlbumDetail extends Album {
  photos: Photo[]
}

export interface AlbumDraft {
  name: string
  description: string
  displayCreatedDate: string | null
  photoDateStart: string | null
  photoDateEnd: string | null
}

export interface SourceDeletionFailure {
  photoId: string
  error: string
}

export interface PhotoDeletionResult {
  deleted: number
  objectsRemoved: number
  cleanupPending: number
  failures: SourceDeletionFailure[]
}

export interface AlbumDeletionResult {
  deleted: boolean
  photosDeleted: number
  objectsRemoved: number
  cleanupPending: number
  failures: SourceDeletionFailure[]
}

/* ------------------------------- 存储与任务 ------------------------------ */

export interface StorageMigrationJob {
  id: string
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled' | 'interrupted'
  sourceBackend: StorageBackend
  targetBackend: StorageBackend
  total: number
  completed: number
  succeeded: number
  failed: number
  cancelled: number
  cleanupStatus:
    | 'not_ready' | 'pending' | 'cleaning' | 'cleaned'
    | 'retained' | 'failed' | 'interrupted'
  cleanupCompleted: number
  cleanupFailed: number
  createdAt: number
  updatedAt: number
  activatedAt: number | null
  error: string | null
}

export interface ThumbnailRebuildJob {
  id: string
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled' | 'interrupted'
  phase: 'queued' | 'clearing' | 'generating'
  total: number
  completed: number
  succeeded: number
  failed: number
  skipped: number
  cancelled: number
  cacheFilesRemoved: number
  workerCount: number
  createdAt: number
  updatedAt: number
  error: string | null
}

export interface S3CleanupJob {
  id: string
  status: 'running' | 'ready' | 'completed' | 'failed' | 'cancelled' | 'interrupted'
  phase: 'scanning' | 'ready' | 'deleting'
  scannedObjects: number
  protectedObjects: number
  total: number
  completed: number
  deleted: number
  failed: number
  skipped: number
  bytesFound: number
  bytesDeleted: number
  workerCount: number
  managedPrefix: string
  createdAt: number
  updatedAt: number
  error: string | null
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

/* ------------------------------- 下载管理 ------------------------------- */

export interface AlbumDownloadSettings {
  albumId: string
  albumName: string
  enabled: boolean
  formats: DownloadFormat[]
  maxImageBytes: number
  maxZipBytes: number
  revision: number
}

export interface AlbumDownloadJob {
  id: string
  albumId: string
  albumName: string
  format: DownloadFormat
  revision: number
  status: string
  total: number
  completed: number
  byteSize: number
  error: string | null
  createdAt: number
  updatedAt: number
}

export interface AdminAlbumDownloads {
  settings: AlbumDownloadSettings[]
  jobs: AlbumDownloadJob[]
  localBytes: number
  directory: string
}

/* --------------------------------- 鉴权 --------------------------------- */

export interface AuthStatusResponse {
  initialized: boolean
  authenticated: boolean
  username?: string
}

export interface AdminAuthState {
  checked: boolean
  loading: boolean
  initialized: boolean
  authenticated: boolean
  username: string
  error: string
}
