import { FormEvent, useCallback, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import './styles.css'

type Album = { id: string; name: string; createdAt: number; photoCount: number }
type Photo = { id: string; albumId: string; originalName: string; format: string; byteSize: number; createdAt: number }
type Job = {
  id: string
  status: string
  targetFormat: string
  total: number
  completed: number
  succeeded: number
  failed: number
  cancelled: number
  createdAt: number
  updatedAt: number
  sourcesDeletedAt: number | null
}
type JobItem = { id: string; sourcePhotoId: string; sourceName: string; status: string; targetPhotoId?: string; error?: string }
type JobResponse = { job: Job; items: JobItem[] }
type DisplayedJob = JobResponse & { itemsLoaded: boolean }
type StorageSettings = {
  backend: 'local' | 'webdav' | 's3'
  localPath: string
  webdavUrl: string
  webdavUsername: string
  webdavPrefix: string
  webdavPassword: string
  webdavPasswordSet: boolean
  s3Endpoint: string
  s3Region: string
  s3Bucket: string
  s3AccessKey: string
  s3SecretKey: string
  s3SecretKeySet: boolean
  s3Prefix: string
}

const INITIAL_STORAGE: StorageSettings = {
  backend: 'local',
  localPath: './data/storage',
  webdavUrl: '',
  webdavUsername: '',
  webdavPrefix: 'chronoframe',
  webdavPassword: '',
  webdavPasswordSet: false,
  s3Endpoint: '',
  s3Region: 'us-east-1',
  s3Bucket: '',
  s3AccessKey: '',
  s3SecretKey: '',
  s3SecretKeySet: false,
  s3Prefix: 'chronoframe',
}

const ACTIVE_JOB_STATUSES = new Set(['queued', 'running'])
const isActiveJob = (status?: string) => Boolean(status && ACTIVE_JOB_STATUSES.has(status))
const statusLabel = (status: string) => ({
  queued: '排队中',
  running: '进行中',
  completed: '完成',
  cancelled: '已安全中断',
  interrupted: '服务器重启后中断',
  failed: '失败',
  processing: '处理中',
  succeeded: '成功',
}[status] ?? status)
const formatDate = (unix: number) => new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium' }).format(unix * 1000)
const formatDateTime = (unix: number) => new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'short' }).format(unix * 1000)
const bytes = (size: number) => size < 1024 * 1024 ? `${Math.ceil(size / 1024)} KB` : `${(size / 1024 / 1024).toFixed(1)} MB`
const clearStorageSecrets = (settings: StorageSettings): StorageSettings => ({ ...settings, webdavPassword: '', s3SecretKey: '' })
const errorMessage = (error: unknown) => error instanceof Error ? error.message : '请求失败'
const isAbortError = (error: unknown) => error instanceof DOMException && error.name === 'AbortError'

async function responseError(response: Response) {
  const body = await response.json().catch(() => ({})) as { error?: string }
  return new Error(body.error ?? `请求失败 (${response.status})`)
}

function App() {
  const [albums, setAlbums] = useState<Album[]>([])
  const [albumsLoaded, setAlbumsLoaded] = useState(false)
  const [albumsError, setAlbumsError] = useState('')
  const [activeAlbum, setActiveAlbum] = useState<Album | null>(null)
  const [albumLoading, setAlbumLoading] = useState(false)
  const [albumError, setAlbumError] = useState('')
  const [photos, setPhotos] = useState<Photo[]>([])
  const [newAlbumOpen, setNewAlbumOpen] = useState(false)
  const [newAlbumName, setNewAlbumName] = useState('')
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [token, setToken] = useState(() => sessionStorage.getItem('chronoframe-admin-token') ?? '')
  const [jobs, setJobs] = useState<Job[]>([])
  const [job, setJob] = useState<DisplayedJob | null>(null)
  const [jobPanelOpen, setJobPanelOpen] = useState(false)
  const [storage, setStorage] = useState<StorageSettings>(INITIAL_STORAGE)
  const [storageOpen, setStorageOpen] = useState(false)
  const [storageTesting, setStorageTesting] = useState(false)
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)

  const albumRequestSequence = useRef(0)
  const activeAlbumRef = useRef<Album | null>(null)

  const request = useCallback(async (path: string, init: RequestInit = {}) => {
    const nextHeaders = new Headers(init.headers)
    if (token) nextHeaders.set('X-Admin-Token', token)
    const result = await fetch(path, { ...init, headers: nextHeaders })
    if (!result.ok) throw await responseError(result)
    return result
  }, [token])

  const loadAlbums = useCallback(async () => {
    try {
      const result = await fetch('/api/albums')
      if (!result.ok) throw await responseError(result)
      const nextAlbums = await result.json() as Album[]
      setAlbums(nextAlbums)
      setAlbumsLoaded(true)
      setAlbumsError('')
      setActiveAlbum(current => {
        if (!current) return null
        const refreshed = nextAlbums.find(album => album.id === current.id)
        if (refreshed) activeAlbumRef.current = refreshed
        return refreshed ?? current
      })
      return nextAlbums
    } catch (error) {
      setAlbumsLoaded(true)
      setAlbumsError(errorMessage(error))
      throw error
    }
  }, [])

  const openAlbum = useCallback(async (
    album: Album,
    options: { resetSelection?: boolean; reportError?: boolean } = {},
  ) => {
    const requestSequence = ++albumRequestSequence.current
    activeAlbumRef.current = album
    setActiveAlbum(album)
    setPhotos([])
    setAlbumError('')
    setAlbumLoading(true)
    if (options.resetSelection ?? true) setSelected(new Set())
    try {
      const result = await fetch(`/api/albums/${album.id}/photos`)
      if (!result.ok) throw await responseError(result)
      const nextPhotos = await result.json() as Photo[]
      if (requestSequence === albumRequestSequence.current) {
        setPhotos(nextPhotos)
        setAlbumLoading(false)
      }
      return true
    } catch (error) {
      if (requestSequence === albumRequestSequence.current) {
        const message = errorMessage(error)
        setPhotos([])
        setAlbumLoading(false)
        setAlbumError(message)
        if (options.reportError ?? true) setNotice(message)
      }
      return false
    }
  }, [])

  const upsertJob = useCallback((nextJob: Job) => {
    setJobs(current => [nextJob, ...current.filter(item => item.id !== nextJob.id)]
      .sort((left, right) => right.createdAt - left.createdAt)
      .slice(0, 100))
  }, [])

  const fetchJobDetail = useCallback(async (jobId: string, signal?: AbortSignal): Promise<DisplayedJob> => {
    const result = await request(`/api/conversions/${jobId}`, { signal })
    const detail = await result.json() as JobResponse
    return { ...detail, itemsLoaded: true }
  }, [request])

  const loadJobs = useCallback(async () => {
    if (!token) {
      setJobs([])
      return []
    }
    const result = await request('/api/conversions')
    const nextJobs = await result.json() as Job[]
    setJobs(nextJobs)
    setJob(current => {
      if (!current) return null
      const summary = nextJobs.find(item => item.id === current.job.id)
      if (!summary) return null
      const justFinished = isActiveJob(current.job.status) && !isActiveJob(summary.status)
      return {
        job: summary,
        items: justFinished ? [] : current.items,
        itemsLoaded: justFinished ? false : current.itemsLoaded,
      }
    })
    return nextJobs
  }, [request, token])

  useEffect(() => {
    void loadAlbums().catch(error => setNotice(errorMessage(error)))
  }, [loadAlbums])

  useEffect(() => {
    if (!token) {
      setJobs([])
      setJob(null)
      setJobPanelOpen(false)
      return
    }
    // A changed credential must not leave another session's task data or poller on screen.
    setJobs([])
    setJob(null)
    setJobPanelOpen(false)
    let cancelled = false
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const result = await request('/api/conversions')
          const nextJobs = await result.json() as Job[]
          if (cancelled) return
          setJobs(nextJobs)
          const rememberedId = sessionStorage.getItem('chronoframe-selected-job')
          const restored = nextJobs.find(item => item.id === rememberedId) ?? nextJobs.find(item => isActiveJob(item.status))
          if (restored) {
            sessionStorage.setItem('chronoframe-selected-job', restored.id)
            setJob({ job: restored, items: [], itemsLoaded: false })
            setJobPanelOpen(true)
          }
        } catch (error) {
          if (!cancelled && !isAbortError(error)) setNotice(errorMessage(error))
        }
      })()
    }, 250)
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [request, token])

  const selectedJobId = job?.job.id
  const selectedJobIsActive = isActiveJob(job?.job.status)
  const selectedJobItemsLoaded = job?.itemsLoaded ?? false

  useEffect(() => {
    if (!selectedJobId || !selectedJobIsActive) return
    let stopped = false
    let timer: number | undefined
    const controller = new AbortController()

    const poll = async () => {
      try {
        const result = await request(`/api/conversions/${selectedJobId}?items=false`, { signal: controller.signal })
        const summary = await result.json() as JobResponse
        if (stopped) return
        upsertJob(summary.job)
        if (isActiveJob(summary.job.status)) {
          setJob(current => current?.job.id === selectedJobId
            ? { job: summary.job, items: [], itemsLoaded: false }
            : current)
          timer = window.setTimeout(poll, 700)
          return
        }

        setJob(current => current?.job.id === selectedJobId
          ? { job: summary.job, items: [], itemsLoaded: false }
          : current)
        void loadAlbums().catch(error => setNotice(errorMessage(error)))
        const currentAlbum = activeAlbumRef.current
        if (currentAlbum) void openAlbum(currentAlbum, { resetSelection: false, reportError: false })
      } catch (error) {
        if (stopped || isAbortError(error)) return
        setNotice(errorMessage(error))
        timer = window.setTimeout(poll, 1800)
      }
    }

    timer = window.setTimeout(poll, 700)
    return () => {
      stopped = true
      controller.abort()
      if (timer !== undefined) window.clearTimeout(timer)
    }
  }, [loadAlbums, openAlbum, request, selectedJobId, selectedJobIsActive, upsertJob])

  useEffect(() => {
    if (!selectedJobId || selectedJobIsActive || selectedJobItemsLoaded) return
    const controller = new AbortController()
    let timer: number | undefined
    const loadDetail = async () => {
      try {
        const detail = await fetchJobDetail(selectedJobId, controller.signal)
        setJob(current => current?.job.id === selectedJobId ? detail : current)
        upsertJob(detail.job)
      } catch (error) {
        if (controller.signal.aborted || isAbortError(error)) return
        setNotice(errorMessage(error))
        timer = window.setTimeout(loadDetail, 2500)
      }
    }
    void loadDetail()
    return () => {
      controller.abort()
      if (timer !== undefined) window.clearTimeout(timer)
    }
  }, [fetchJobDetail, selectedJobId, selectedJobIsActive, selectedJobItemsLoaded, upsertJob])

  const saveToken = (value: string) => {
    setToken(value)
    sessionStorage.setItem('chronoframe-admin-token', value)
    setNotice('管理员令牌已保存在当前浏览器会话中。')
  }

  const loadStorage = async () => {
    setStorage(current => clearStorageSecrets(current))
    try {
      const data = await (await request('/api/settings/storage')).json() as Partial<StorageSettings>
      setStorage(current => clearStorageSecrets({ ...current, ...data }))
      setStorageOpen(true)
    } catch (error) {
      setNotice(errorMessage(error))
    }
  }

  const saveStorage = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const payload = { ...storage }
    setStorage(current => clearStorageSecrets(current))
    setBusy(true)
    try {
      const result = await request('/api/settings/storage', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      })
      const data = await result.json() as Partial<StorageSettings>
      setStorage(current => clearStorageSecrets({ ...current, ...data }))
      setNotice('存储设置已保存，后续上传和转换将使用新设置。')
    } catch (error) {
      setNotice(errorMessage(error))
    } finally {
      setStorage(current => clearStorageSecrets(current))
      setBusy(false)
    }
  }

  const testStorage = async () => {
    setBusy(true)
    setStorageTesting(true)
    try {
      const result = await request('/api/settings/storage/test', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(storage),
      })
      const body = await result.json().catch(() => ({})) as { message?: string }
      setNotice(body.message ?? '存储连接测试成功。')
    } catch (error) {
      setNotice(errorMessage(error))
    } finally {
      setStorageTesting(false)
      setBusy(false)
    }
  }

  const createAlbum = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const name = newAlbumName.trim()
    if (!name) return
    setBusy(true)
    try {
      const result = await request('/api/albums', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name }),
      })
      const album = await result.json() as Album
      setNewAlbumName('')
      setNewAlbumOpen(false)
      await loadAlbums()
      await openAlbum(album)
    } catch (error) {
      setNotice(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  const upload = async (event: FormEvent<HTMLInputElement>) => {
    const album = activeAlbum
    if (!album || !event.currentTarget.files?.length) return
    const data = new FormData()
    Array.from(event.currentTarget.files).forEach(file => data.append('files', file))
    event.currentTarget.value = ''
    setBusy(true)

    let uploadFailure = ''
    try {
      await request(`/api/albums/${album.id}/photos`, { method: 'POST', body: data })
    } catch (error) {
      uploadFailure = errorMessage(error)
    }

    const currentAlbum = activeAlbumRef.current
    const [albumsResult, photosResult] = await Promise.allSettled([
      loadAlbums(),
      currentAlbum?.id === album.id
        ? openAlbum(currentAlbum, { resetSelection: false, reportError: false })
        : Promise.resolve(true),
    ])
    const refreshFailed = albumsResult.status === 'rejected'
      || photosResult.status === 'rejected'
      || (photosResult.status === 'fulfilled' && !photosResult.value)
    if (uploadFailure) {
      setNotice(`${uploadFailure}${refreshFailed ? '；相簿刷新也失败，请稍后重试。' : ''}`)
    } else {
      setNotice(refreshFailed ? '图片已保存，但相簿刷新失败，请重新打开相簿。' : '图片已保存至相簿。')
    }
    setBusy(false)
  }

  const chooseJob = (summary: Job) => {
    sessionStorage.setItem('chronoframe-selected-job', summary.id)
    setJobPanelOpen(true)
    setJob(current => current?.job.id === summary.id
      ? { ...current, job: summary }
      : { job: summary, items: [], itemsLoaded: false })
  }

  const convert = async (targetFormat: string) => {
    if (!selected.size) {
      setNotice('请先在左侧选择一个或多个相簿。')
      return
    }
    setBusy(true)
    try {
      const result = await request('/api/conversions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ albumIds: [...selected], targetFormat }),
      })
      const nextJob = await result.json() as Job
      upsertJob(nextJob)
      chooseJob(nextJob)
      setNotice('转换任务已进入后台队列；可以继续浏览和上传。')
    } catch (error) {
      setNotice(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  const cancel = async () => {
    if (!job) return
    setBusy(true)
    try {
      await request(`/api/conversions/${job.job.id}/cancel`, { method: 'POST' })
      setNotice('已请求安全中断：未开始的图片会取消，已经提交的图片会保留。')
    } catch (error) {
      setNotice(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  const removeSources = async () => {
    if (!job || job.job.sourcesDeletedAt !== null
      || !window.confirm('确认删除此任务已成功转换的旧格式原图？这不会删除转换出的新图。')) return
    const jobId = job.job.id
    setBusy(true)
    try {
      const result = await request(`/api/conversions/${jobId}/delete-sources`, { method: 'DELETE' })
      const body = await result.json() as { removed: number; failures: unknown[] }
      const currentAlbum = activeAlbumRef.current
      const [detailResult, albumsResult, photosResult] = await Promise.allSettled([
        fetchJobDetail(jobId),
        loadAlbums(),
        currentAlbum ? openAlbum(currentAlbum, { resetSelection: false, reportError: false }) : Promise.resolve(true),
      ])
      if (detailResult.status === 'fulfilled') {
        upsertJob(detailResult.value.job)
        setJob(current => current?.job.id === jobId ? detailResult.value : current)
      }
      const refreshFailed = detailResult.status === 'rejected'
        || albumsResult.status === 'rejected'
        || photosResult.status === 'rejected'
        || (photosResult.status === 'fulfilled' && !photosResult.value)
      setNotice(`已删除 ${body.removed} 张旧格式图片${body.failures.length ? `，${body.failures.length} 张删除失败，可稍后重试` : ''}${refreshFailed ? '；部分页面状态刷新失败' : ''}。`)
    } catch (error) {
      setNotice(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  const failedItems = job?.items.filter(item => item.status === 'failed' || Boolean(item.error)) ?? []

  return <main>
    <aside>
      <div className="brand"><span>◒</span><div><strong>ChronoFrame</strong><small>相簿优先的私人画廊</small></div></div>
      <button className="primary" onClick={() => setNewAlbumOpen(open => !open)} disabled={busy}>＋ 新建相簿</button>
      {newAlbumOpen && <form className="new-album-form" onSubmit={createAlbum}>
        <label>相簿名<input aria-label="新相簿名" autoFocus maxLength={100} value={newAlbumName} onChange={event => setNewAlbumName(event.target.value)} placeholder="例如：2026 夏日" required /></label>
        <div><button type="button" className="secondary" onClick={() => { setNewAlbumOpen(false); setNewAlbumName('') }}>取消</button><button type="submit" className="primary" disabled={busy || !newAlbumName.trim()}>创建</button></div>
      </form>}
      <section>
        <div className="section-title"><span>相簿空间</span><span>{albums.length}</span></div>
        {albumsLoaded && !albumsError && albums.length === 0 && <p className="empty-side">从一个相簿开始，再把照片放进来。</p>}
        {albumsError && <p className="empty-side error-text">相簿载入失败，请稍后重试。</p>}
        {albums.map(album => <div key={album.id} className={`album-row ${activeAlbum?.id === album.id ? 'active' : ''}`}>
          <input aria-label={`选择 ${album.name}`} type="checkbox" checked={selected.has(album.id)} onChange={() => setSelected(old => {
            const next = new Set(old)
            next.has(album.id) ? next.delete(album.id) : next.add(album.id)
            return next
          })} />
          <button onClick={() => void openAlbum(album)}><span className="album-icon">▣</span><span>{album.name}</span><em>{album.photoCount}</em></button>
        </div>)}
      </section>
      <section className="convert">
        <div className="section-title"><span>批量格式转换</span></div>
        <p>选中相簿后，后台安全转换全部支持格式的图片。</p>
        <div className="format-buttons">{['PNG', 'JPG', 'JPEG', 'WEBP'].map(format => <button key={format} disabled={busy || selected.size === 0} onClick={() => void convert(format)}>{format}</button>)}</div>
      </section>
      <section className="jobs">
        <div className="section-title"><span>任务中心</span><button aria-label="刷新转换任务" title="刷新转换任务" disabled={!token || busy} onClick={() => void loadJobs().catch(error => setNotice(errorMessage(error)))}>↻</button></div>
        {!token ? <p className="empty-side">填写管理员令牌后，可恢复和查看转换任务。</p> : jobs.length === 0 ? <p className="empty-side">还没有转换任务。</p> : <div className="job-list">
          {jobs.map(summary => <button key={summary.id} className={job?.job.id === summary.id ? 'active' : ''} aria-pressed={job?.job.id === summary.id} onClick={() => chooseJob(summary)}>
            <span><strong>{summary.targetFormat.toUpperCase()} 转换</strong><small>{formatDate(summary.createdAt)} · {statusLabel(summary.status)}{summary.sourcesDeletedAt !== null && summary.sourcesDeletedAt < 0 ? ' · 正在删除原图' : summary.sourcesDeletedAt !== null ? ' · 原图已删除' : ''}</small></span>
            <em>{summary.completed}/{summary.total}</em>
          </button>)}
        </div>}
      </section>
      <section className="auth">
        <label>管理员令牌<input type="password" value={token} placeholder="CF_ADMIN_TOKEN" onChange={event => saveToken(event.target.value)} /></label>
        <small>令牌仅存储在本次浏览器会话。</small>
        <button className="storage-toggle" onClick={() => void loadStorage()} disabled={!token || busy}>存储设置</button>
        {storageOpen && <form className="storage-form" onSubmit={saveStorage}>
          <label>存储后端<select value={storage.backend} onChange={event => setStorage(current => ({ ...current, backend: event.target.value as StorageSettings['backend'] }))}><option value="local">本地磁盘</option><option value="webdav">WebDAV</option><option value="s3">S3 兼容存储</option></select></label>
          {storage.backend === 'local' ? <label>存储路径<input value={storage.localPath} onChange={event => setStorage(current => ({ ...current, localPath: event.target.value }))} required /></label> : storage.backend === 'webdav' ? <>
            <label>WebDAV 地址<input type="url" value={storage.webdavUrl} placeholder="https://dav.example.com/.../" onChange={event => setStorage(current => ({ ...current, webdavUrl: event.target.value }))} required /></label>
            <label>用户名<input value={storage.webdavUsername} onChange={event => setStorage(current => ({ ...current, webdavUsername: event.target.value }))} required /></label>
            <label>密码 / 应用密码<input type="password" placeholder={storage.webdavPasswordSet ? '已保存；留空则不变' : '首次配置必填'} value={storage.webdavPassword} onChange={event => setStorage(current => ({ ...current, webdavPassword: event.target.value }))} required={!storage.webdavPasswordSet} /></label>
            <label>远端目录前缀<input value={storage.webdavPrefix} onChange={event => setStorage(current => ({ ...current, webdavPrefix: event.target.value }))} /></label>
          </> : <>
            <label>S3 Endpoint<input type="url" value={storage.s3Endpoint} placeholder="https://s3.example.com" onChange={event => setStorage(current => ({ ...current, s3Endpoint: event.target.value }))} required /></label>
            <label>区域<input value={storage.s3Region} onChange={event => setStorage(current => ({ ...current, s3Region: event.target.value }))} required /></label>
            <label>桶名<input value={storage.s3Bucket} onChange={event => setStorage(current => ({ ...current, s3Bucket: event.target.value }))} required /></label>
            <label>访问密钥<input value={storage.s3AccessKey} onChange={event => setStorage(current => ({ ...current, s3AccessKey: event.target.value }))} required /></label>
            <label>秘密访问密钥<input type="password" placeholder={storage.s3SecretKeySet ? '已保存；留空则不变' : '首次配置必填'} value={storage.s3SecretKey} onChange={event => setStorage(current => ({ ...current, s3SecretKey: event.target.value }))} required={!storage.s3SecretKeySet} /></label>
            <label>对象前缀<input value={storage.s3Prefix} onChange={event => setStorage(current => ({ ...current, s3Prefix: event.target.value }))} /></label>
          </>}
          <div className="storage-actions">
            <button type="button" className="secondary" disabled={busy} onClick={event => {
              if (event.currentTarget.form?.reportValidity()) void testStorage()
            }}>{storageTesting ? '测试中…' : '测试连接'}</button>
            <button type="submit" className="primary" disabled={busy}>保存设置</button>
          </div>
        </form>}
      </section>
    </aside>
    <section className="content">
      {notice && <div className="notice"><span>{notice}</span><button aria-label="关闭通知" onClick={() => setNotice('')}>×</button></div>}
      {!activeAlbum ? !albumsLoaded ? <div className="welcome"><div className="welcome-art">▦</div><h1>正在载入相簿</h1><p>正在读取你的相簿空间。</p></div> : albumsError ? <div className="welcome"><div className="welcome-art">▦</div><h1>无法载入相簿</h1><p>{albumsError}</p><button className="primary" onClick={() => void loadAlbums().catch(error => setNotice(errorMessage(error)))}>重新载入</button></div> : albums.length === 0 ? <div className="welcome"><div className="welcome-art">▦</div><h1>你的相簿空间</h1><p>先创建一个相簿，再上传图片。这里不会展示一个混杂的“所有图片”页面。</p><button className="primary" onClick={() => setNewAlbumOpen(true)}>创建第一个相簿</button></div> : <div className="welcome"><div className="welcome-art">▦</div><h1>选择一个相簿</h1><p>左侧已有 {albums.length} 个相簿。选择一个相簿即可浏览和上传图片。</p><button className="primary" onClick={() => void openAlbum(albums[0])}>打开最近的相簿</button></div> : <>
        <header><div><p className="eyebrow">相簿</p><h1>{activeAlbum.name}</h1><p>{photos.length} 张图片 · 创建于 {formatDate(activeAlbum.createdAt)}</p></div><label className={`upload ${busy ? 'disabled' : ''}`}>上传图片<input type="file" multiple accept="image/png,image/jpeg,image/webp" onChange={upload} disabled={busy} /></label></header>
        {albumLoading ? <div className="empty"><div>▧</div><h2>正在载入图片</h2><p>请稍候。</p></div> : albumError ? <div className="empty"><div>▧</div><h2>无法载入这个相簿</h2><p>{albumError}</p><button className="secondary" onClick={() => void openAlbum(activeAlbum, { resetSelection: false })}>重新载入</button></div> : photos.length === 0 ? <div className="empty"><div>▧</div><h2>这个相簿还是空的</h2><p>PNG、JPG/JPEG 和 WEBP 图片都可以直接上传。</p></div> : <div className="photo-grid">{photos.map(photo => <figure key={photo.id}><img loading="lazy" src={`/api/photos/${photo.id}/file`} alt={photo.originalName} /><figcaption><span title={photo.originalName}>{photo.originalName}</span><small>{photo.format.toUpperCase()} · {bytes(photo.byteSize)}</small></figcaption></figure>)}</div>}
      </>}
    </section>
    {job && jobPanelOpen && <section className="job-panel" aria-live="polite">
      <div className="job-heading"><div><p className="eyebrow">后台转换</p><h2>{job.job.targetFormat.toUpperCase()} 转换任务</h2></div><button className="close" aria-label="关闭任务面板" onClick={() => setJobPanelOpen(false)}>×</button></div>
      <div className="progress"><i style={{ width: `${job.job.total ? job.job.completed / job.job.total * 100 : 0}%` }} /></div>
      <p><strong>{job.job.completed} / {job.job.total}</strong> 已处理 · 成功 {job.job.succeeded} · 失败 {job.job.failed} · 取消 {job.job.cancelled}</p>
      <p className="status">状态：{statusLabel(job.job.status)}</p>
      {job.job.sourcesDeletedAt !== null && job.job.sourcesDeletedAt < 0 ? <p className="source-state">旧格式原图：正在安全删除</p> : job.job.sourcesDeletedAt !== null ? <p className="source-state deleted">旧格式原图：已于 {formatDateTime(job.job.sourcesDeletedAt)} 删除</p> : job.job.succeeded > 0 ? <p className="source-state">旧格式原图：仍保留</p> : null}
      <div className="job-actions">
        {isActiveJob(job.job.status) && <button disabled={busy} onClick={() => void cancel()}>安全中断任务</button>}
        {!isActiveJob(job.job.status) && job.job.succeeded > 0 && job.job.sourcesDeletedAt === null && <button className="danger" disabled={busy} onClick={() => void removeSources()}>确认删除旧格式原图</button>}
      </div>
      {!isActiveJob(job.job.status) && !job.itemsLoaded && <p className="detail-loading">正在载入每张图片的处理结果…</p>}
      {failedItems.length > 0 && <details className="job-issues" open><summary>失败 / 异常项目（{failedItems.length}）</summary><ul>{failedItems.map(item => <li key={item.id}><strong>{item.sourceName || item.sourcePhotoId || '未知图片'}</strong><span>{statusLabel(item.status)}{item.error ? ` · ${item.error}` : ''}</span></li>)}</ul></details>}
      <details><summary>中断与数据安全</summary><p>任务按图片独立处理，输出先写入临时对象后原子提交。取消会停止未开始的项目；已完成提交的转换图会保留，原图始终保留直到管理员明确确认删除。服务器重启会将未完成任务标记为“已中断”，不会自动恢复或误删文件。</p></details>
    </section>}
  </main>
}

createRoot(document.getElementById('root')!).render(<App />)
