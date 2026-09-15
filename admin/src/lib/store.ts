// 跨页面共享的应用级状态。
//
// 对应 Vue 侧的 Nuxt `useState(key)` 单例。这些状态必须**跨路由存活**：
// 上传队列在切换后台页面时不能中断，相册列表的搜索词/页码/勾选在从工作区
// 返回列表时要还在。因此它们存放在模块级，而不是任何组件的 state 里。
// 浏览器刷新即重置——这是设计而非缺陷，File 句柄无法序列化，无法持久化。
import { useSyncExternalStore } from 'react'
import { adminApi, getAdminApiErrorMessage } from './api'
import { setThemePreference } from './theme'
import { createUploadQueue, createUploadQueueState } from './upload-queue'
import type { UploadItem, UploadQueue, UploadQueueState } from './upload-queue'
import type { SiteSettings } from './types'

/* ------------------------------ 微型 store ------------------------------ */

interface Store<T> {
  get: () => T
  set: (next: T) => void
  update: (updater: (previous: T) => T) => void
  subscribe: (listener: () => void) => () => void
  getVersion: () => number
}

function createStore<T>(initial: T): Store<T> {
  let state = initial
  let version = 0
  const listeners = new Set<() => void>()
  const emit = () => {
    version += 1
    for (const listener of listeners) listener()
  }
  return {
    get: () => state,
    set: (next) => { state = next; emit() },
    update: (updater) => { state = updater(state); emit() },
    subscribe: (listener) => {
      listeners.add(listener)
      return () => { listeners.delete(listener) }
    },
    getVersion: () => version,
  }
}

function useStore<T>(store: Store<T>): T {
  useSyncExternalStore(store.subscribe, store.getVersion, store.getVersion)
  return store.get()
}

/* ------------------------------ 站点设置 ------------------------------- */

export const DEFAULT_SITE_SETTINGS: SiteSettings = {
  title: 'Open Gallery',
  slogan: 'Frame the moments that matter.',
  author: 'Open Gallery',
  avatarUrl: '/web-app-manifest-192x192.png',
  theme: 'system',
}

const siteSettingsStore = createStore<SiteSettings>({ ...DEFAULT_SITE_SETTINGS })
let siteSettingsLoaded = false
let pendingSiteSettings: Promise<SiteSettings> | null = null

export const normalizeSiteSettings = (value: SiteSettings): SiteSettings => ({
  title: value.title?.trim() || DEFAULT_SITE_SETTINGS.title,
  slogan: value.slogan?.trim() || '',
  author: value.author?.trim() || '',
  avatarUrl: value.avatarUrl?.trim() || DEFAULT_SITE_SETTINGS.avatarUrl,
  theme: ['light', 'dark', 'system'].includes(value.theme) ? value.theme : 'system',
})

export const applySiteSettings = (value: SiteSettings): SiteSettings => {
  const normalized = normalizeSiteSettings(value)
  siteSettingsLoaded = true
  siteSettingsStore.set(normalized)
  // 站点默认主题同时决定后台外观，与现网「保存后本机立即变色」的行为一致。
  setThemePreference(normalized.theme)
  return normalized
}

/** GET /api/settings/site 是公开端点，不需要登录，因此走 request 而不是 adminFetch。 */
export const ensureSiteSettings = async (): Promise<SiteSettings> => {
  if (siteSettingsLoaded) return siteSettingsStore.get()
  if (pendingSiteSettings) return await pendingSiteSettings
  pendingSiteSettings = adminApi
    .request<SiteSettings>('/api/settings/site')
    .then(applySiteSettings)
    .finally(() => { pendingSiteSettings = null })
  return await pendingSiteSettings
}

export const useSiteSettings = (): SiteSettings => useStore(siteSettingsStore)

/* ------------------------------ 上传队列 ------------------------------- */

const uploadState: UploadQueueState<File> = createUploadQueueState<File>()
let uploadVersion = 0
const uploadListeners = new Set<() => void>()
const emitUploads = () => {
  uploadVersion += 1
  for (const listener of uploadListeners) listener()
}

// emitUploads 作为 notify 交给队列本身：状态变化有一半发生在 Promise 回调里
// （'uploading' → 'done'/'failed'、albumVersions 递增），只在动作函数外层包一层
// 是通知不到的——包在 upload() 的 finally 里更会早一步，此时终态还没写入，
// UI 只能看到"上传中"，队列结束后永远停在那一帧。
const uploadQueue: UploadQueue<File> = createUploadQueue<File>(
  uploadState,
  async (file, albumId) => {
    const body = new FormData()
    // 字段名必须是 `files`（封面单独上传用的是 `file`，两者不同）。
    body.append('files', file)
    return await adminApi.adminFetch(
      `/api/albums/${encodeURIComponent(albumId)}/photos`,
      { method: 'POST', body },
    )
  },
  getAdminApiErrorMessage,
  emitUploads,
)

const uploadDrawerStore = createStore<boolean>(false)

export interface UploadsApi {
  state: UploadQueueState<File>
  queue: UploadQueue<File>
  /** uploading + queued。注意**不含** failed——退出登录与 beforeunload 的判定都依赖这一点。 */
  pending: number
  active: number
  queued: number
  done: number
  failed: number
  items: UploadItem<File>[]
  open: boolean
  setOpen: (open: boolean) => void
}

export const useUploads = (): UploadsApi => {
  useSyncExternalStore(
    (listener) => {
      uploadListeners.add(listener)
      return () => { uploadListeners.delete(listener) }
    },
    () => uploadVersion,
    () => uploadVersion,
  )
  const open = useStore(uploadDrawerStore)

  const items = uploadState.items
  const active = items.filter((item) => item.status === 'uploading').length
  const queued = items.filter((item) => item.status === 'queued').length

  return {
    state: uploadState,
    queue: uploadQueue,
    items,
    active,
    queued,
    done: items.filter((item) => item.status === 'done').length,
    failed: items.filter((item) => item.status === 'failed').length,
    pending: active + queued,
    open,
    setOpen: (value) => uploadDrawerStore.set(value),
  }
}

/** 非组件环境（如退出登录拦截）读取队列计数。 */
export const uploadSnapshot = () => {
  const items = uploadState.items
  const active = items.filter((item) => item.status === 'uploading').length
  const queued = items.filter((item) => item.status === 'queued').length
  const failed = items.filter((item) => item.status === 'failed').length
  return { active, queued, failed, pending: active + queued, total: items.length }
}

/** 相册内该相册尚未结束的上传数，用于阻止删除相册。 */
export const albumPendingUploads = (albumId: string): number =>
  uploadState.items.filter(
    (item) => item.albumId === albumId
      && (item.status === 'queued' || item.status === 'uploading'),
  ).length

/* --------------------------- 相册列表视图状态 ---------------------------- */

export interface AlbumListViewState {
  query: string
  page: number
  selected: string[]
}

const albumListViewStore = createStore<AlbumListViewState>({ query: '', page: 1, selected: [] })

export const useAlbumListView = () => {
  const value = useStore(albumListViewStore)
  return {
    value,
    set: albumListViewStore.set,
    update: albumListViewStore.update,
  }
}

/* ----------------------------- 图片视图模式 ----------------------------- */

export type PhotoView = 'grid' | 'table'
const photoViewStore = createStore<PhotoView>('grid')
export const usePhotoView = () => {
  const value = useStore(photoViewStore)
  return { value, set: photoViewStore.set }
}
