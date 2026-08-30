export type DownloadFormat = 'png' | 'jpg' | 'jpeg' | 'webp'
export interface PublicAlbumDownload {
  albumId: string
  formats: Array<{ format: DownloadFormat; status: string; byteSize: number; url: string | null }>
}
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
