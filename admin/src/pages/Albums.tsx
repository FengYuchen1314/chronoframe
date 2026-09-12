import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import {
  Alert, Button, Card, Checkbox, Chip, Input, Label, Modal, Radio, RadioGroup,
  Tabs, TextArea, TextField,
} from '@heroui/react'
import { adminApi, getAdminApiErrorMessage } from '../lib/api'
import {
  ALBUM_UPLOAD_ACCEPT, albumDraftOf, isAlbumUploadable, toggleVisibleSelection, validateAlbumDraft,
} from '../lib/albums'
import { adminBytes } from '../lib/format'
import { notice } from '../lib/notice'
import { useBeforeUnload, useLeaveGuard } from '../lib/navigation'
import { albumPendingUploads, useAlbumListView, usePhotoView, useUploads } from '../lib/store'
import { Icon } from '../lib/icons'
import type {
  Album, AlbumCover, AlbumDeletionResult, AlbumDetail, AlbumDraft, Photo, PhotoDeletionResult,
} from '../lib/types'
import PageHeader from '../components/PageHeader'
import DataTable from '../components/DataTable'
import type { Column } from '../components/DataTable'
import Pager from '../components/Pager'
import AlbumCoverEditor from '../components/AlbumCoverEditor'
import Lightbox from '../components/Lightbox'
import type { LightboxPhoto } from '../components/Lightbox'
import DownloadManager from '../components/DownloadManager'
import { useDocumentTitle } from '../lib/hooks'

const PHOTO_PAGE_SIZE = 48
const ALBUM_PAGE_SIZE = 20
const EMPTY_DRAFT: AlbumDraft = {
  name: '', description: '', displayCreatedDate: null, photoDateStart: null, photoDateEnd: null,
}
const PHOTO_FORMAT_OPTIONS = [
  { value: 'all', label: '全部格式' },
  { value: 'png', label: 'PNG' },
  { value: 'jpg', label: 'JPG / JPEG' },
  { value: 'webp', label: 'WebP' },
]
const PHOTO_SORT_OPTIONS = [
  { value: 'newest', label: '最近上传' },
  { value: 'name', label: '文件名称' },
  { value: 'size', label: '文件大小' },
]

export default function Albums() {
  useDocumentTitle('相册管理')
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const uploads = useUploads()
  const listView = useAlbumListView()
  const photoView = usePhotoView()

  // URL 是唯一真相：?album= 是工作区开关，?tab= 只认 details/downloads
  // （其余值静默回落 photos，但不去清洗 URL 里的非法值）。
  const selectedId = searchParams.get('album') || ''
  const rawTab = String(searchParams.get('tab') || '')
  const tab = ['details', 'downloads'].includes(rawTab) ? rawTab : 'photos'

  const listState = listView.value

  const [albums, setAlbums] = useState<Album[]>([])
  const [photos, setPhotos] = useState<Photo[]>([])
  const [loading, setLoading] = useState(false)
  const [detailLoading, setDetailLoading] = useState(false)
  const [ready, setReady] = useState(false)
  const [error, setError] = useState('')
  const [detailError, setDetailError] = useState('')

  const [mutation, setMutation] = useState('')
  const [coverBusy, setCoverBusy] = useState(false)
  const [downloadBusy, setDownloadBusy] = useState(false)
  const [downloadDirty, setDownloadDirty] = useState(false)

  const [draft, setDraft] = useState<AlbumDraft>(EMPTY_DRAFT)
  const [baseline, setBaseline] = useState('')
  const [formError, setFormError] = useState('')

  const [selectedPhotos, setSelectedPhotos] = useState<string[]>([])
  const [photoQuery, setPhotoQuery] = useState('')
  const [photoFormat, setPhotoFormat] = useState<'all' | 'png' | 'jpg' | 'webp'>('all')
  const [photoSort, setPhotoSort] = useState<'newest' | 'name' | 'size'>('newest')
  const [photoPage, setPhotoPage] = useState(1)
  // 点缩略图打开的预览图。对应现网 AImage 的 preview，选中态与它无关。
  const [preview, setPreview] = useState<LightboxPhoto | null>(null)

  const [orderMode, setOrderMode] = useState(false)
  const [orderIds, setOrderIds] = useState<string[]>([])

  const [createOpen, setCreateOpen] = useState(false)
  const [newAlbum, setNewAlbum] = useState({ name: '', description: '' })
  const [createError, setCreateError] = useState('')

  const [exportOpen, setExportOpen] = useState(false)

  const selectedAlbum = albums.find((album) => album.id === selectedId)
  const locked = Boolean(mutation) || coverBusy || downloadBusy
  const dirty = Boolean(baseline) && JSON.stringify(draft) !== baseline
  const orderDirty = orderMode && orderIds.join() !== albums.map((album) => album.id).join()
  const albumUploads = selectedId ? albumPendingUploads(selectedId) : 0

  const selectedRef = useRef(selectedId)
  const dirtyRef = useRef(dirty)
  const serial = useRef(0)
  const disposed = useRef(false)
  const refreshTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  selectedRef.current = selectedId
  dirtyRef.current = dirty

  /* ------------------------------ 数据加载 ------------------------------ */

  const loadAlbums = useCallback(async () => {
    setLoading(true)
    setError('')
    try {
      const value = await adminApi.adminFetch<Album[]>('/api/albums')
      setAlbums(value)
      // 收缩勾选，剔除已被删除的相册。
      listView.update((previous) => ({
        ...previous,
        selected: previous.selected.filter((id) => value.some((album) => album.id === id)),
      }))
    } catch (cause) {
      setError(getAdminApiErrorMessage(cause))
    } finally {
      setLoading(false)
    }
    // listView 是模块级稳定引用，不需要进依赖。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const loadDetail = useCallback(async (id = selectedRef.current) => {
    if (!id) {
      setDetailLoading(false)
      return
    }
    // 序号守卫：切相册时旧响应整体丢弃，避免用上一个相册的数据覆盖新页面。
    const current = ++serial.current
    setDetailLoading(true)
    try {
      const detail = await adminApi.adminFetch<AlbumDetail>(`/api/albums/${encodeURIComponent(id)}`)
      if (disposed.current || current !== serial.current || selectedRef.current !== id) return
      setAlbums((previous) => {
        const index = previous.findIndex((album) => album.id === detail.id)
        if (index < 0) return [...previous, detail]
        const next = [...previous]
        next[index] = detail
        return next
      })
      setPhotos(detail.photos)
      // 详情返回该相册全量照片，所以同相册内的刷新不会误清选择，只会剔除已删除的图片。
      setSelectedPhotos((previous) =>
        previous.filter((pid) => detail.photos.some((photo) => photo.id === pid)))
      setReady(true)
      setDetailError('')
      // 刷新不覆盖用户正在编辑的草稿。
      if (!dirtyRef.current) {
        const next = albumDraftOf(detail)
        setDraft(next)
        setBaseline(JSON.stringify(next))
      }
    } catch (cause) {
      if (current === serial.current) setDetailError(getAdminApiErrorMessage(cause))
    } finally {
      if (current === serial.current) setDetailLoading(false)
    }
  }, [])

  // 只在挂载时初始化一次；相册切换由下面的专用效果负责。
  useEffect(() => {
    disposed.current = false
    void loadAlbums()
    void loadDetail()
    const timer = refreshTimer
    return () => {
      disposed.current = true
      serial.current += 1
      clearTimeout(timer.current)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // 切相册：重置工作区局部状态并重新拉详情。
  useEffect(() => {
    setBaseline('')
    setReady(false)
    setPhotos([])
    setSelectedPhotos([])
    setOrderMode(false)
    setOrderIds([])
    setDownloadDirty(false)
    setCoverBusy(false)
    setPhotoQuery('')
    setPhotoFormat('all')
    setPhotoPage(1)
    setFormError('')
    void loadDetail(selectedId)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId])

  // 上传成功会递增 albumVersions；用 1 秒去抖做局部刷新，
  // 这样连续上传不会把可见结果一直推迟到整个队列结束。
  const version = uploads.state.albumVersions[selectedId] ?? 0
  useEffect(() => {
    if (!selectedId || !ready) return
    clearTimeout(refreshTimer.current)
    refreshTimer.current = setTimeout(() => { void loadDetail(selectedId) }, 1000)
    return () => clearTimeout(refreshTimer.current)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version])

  /* ------------------------------ 导航与守卫 ---------------------------- */

  const navigateAlbum = (id = '', nextTab = 'photos') => {
    // push（不是 replace）：浏览器后退可以从工作区回到列表。
    if (!id) {
      navigate('/albums')
      return
    }
    navigate(`/albums?album=${encodeURIComponent(id)}${nextTab === 'photos' ? '' : `&tab=${nextTab}`}`)
  }

  // 只有 ?album 变化（切相册或离开工作区）才拦截；同一相册内切页签不拦。
  useLeaveGuard({
    shouldBlock: (next, currentLocation) => {
      if (!locked && !dirty && !downloadDirty && !orderDirty) return false
      if (next.pathname !== currentLocation.pathname) return true
      const nextAlbum = new URLSearchParams(next.search).get('album') || ''
      const currentAlbum = new URLSearchParams(currentLocation.search).get('album') || ''
      return nextAlbum !== currentAlbum
    },
    message: '有未保存的修改，确定放弃修改并离开吗？',
    hardBlock: Boolean(mutation),
  })
  useBeforeUnload(dirty || downloadDirty || orderDirty || locked)

  /* -------------------------------- 相册 -------------------------------- */

  const applyDraft = (album: Album) => {
    const next = albumDraftOf(album)
    setDraft(next)
    setBaseline(JSON.stringify(next))
    setFormError('')
  }

  const create = async () => {
    if (mutation) return
    // 与现网一致：以「编辑草稿」为底、用新建表单覆盖名称与简介，日期强制为 null。
    const message = validateAlbumDraft({
      ...draft,
      ...newAlbum,
      displayCreatedDate: null,
      photoDateStart: null,
      photoDateEnd: null,
    })
    if (message) {
      setCreateError(message)
      return
    }
    setMutation('create')
    try {
      const created = await adminApi.adminFetch<Album>('/api/albums', {
        method: 'POST',
        body: { name: newAlbum.name.trim(), description: newAlbum.description.trim() },
      })
      setAlbums((previous) => [...previous, created])
      setCreateOpen(false)
      setNewAlbum({ name: '', description: '' })
      setCreateError('')
      notice.add({ title: '相册已创建，可以上传图片了', color: 'success' })
      navigateAlbum(created.id)
    } catch (cause) {
      setCreateError(getAdminApiErrorMessage(cause))
    } finally {
      setMutation('')
    }
  }

  const save = async () => {
    if (locked || !ready || !dirty || !selectedAlbum) return
    const message = validateAlbumDraft(draft)
    setFormError(message || '')
    if (message) return
    setMutation('save')
    try {
      const updated = await adminApi.adminFetch<Album>(`/api/albums/${selectedId}`, {
        method: 'PATCH',
        // 全量提交（含 3 个可为 null 的日期字段），不是只发改动字段。
        body: { ...draft, name: draft.name.trim(), description: draft.description.trim() },
      })
      setAlbums((previous) => previous.map((album) => (album.id === updated.id ? updated : album)))
      applyDraft(updated)
      notice.add({ title: '相册资料已保存', color: 'success' })
    } catch (cause) {
      setFormError(getAdminApiErrorMessage(cause))
    } finally {
      setMutation('')
    }
  }

  const removeAlbum = async () => {
    if (locked || dirty || downloadDirty || !selectedAlbum) return
    if (albumUploads > 0) {
      uploads.setOpen(true)
      notice.add({ title: '请先完成或取消此相册的上传队列', color: 'warning' })
      return
    }
    const target = selectedAlbum
    const name = target.name
    setMutation('confirm-delete-album')
    const ok = await notice.confirm(
      `永久删除相册「${name}」及其中全部 ${target.photoCount} 张图片？对应存储文件、封面和本地 ZIP 也会清理，无法撤销。`,
      true,
    )
    if (!ok) {
      setMutation('')
      return
    }
    setMutation('delete-album')
    try {
      const result = await adminApi.adminFetch<AlbumDeletionResult>(
        `/api/albums/${target.id}`,
        { method: 'DELETE' },
      )
      if (result.deleted) {
        setBaseline('')
        setDownloadDirty(false)
        setAlbums((previous) => previous.filter((album) => album.id !== target.id))
        listView.update((previous) => ({
          ...previous,
          selected: previous.selected.filter((id) => id !== target.id),
        }))
        notice.add({
          title: `已删除「${name}」`,
          description: result.cleanupPending ? '剩余存储文件将在后台继续清理' : undefined,
          color: result.cleanupPending ? 'warning' : 'success',
        })
        navigateAlbum()
      }
    } catch (cause) {
      notice.add({
        title: '删除失败，请刷新确认',
        description: getAdminApiErrorMessage(cause),
        color: 'error',
      })
    } finally {
      setMutation('')
    }
  }

  /* -------------------------------- 排序 -------------------------------- */

  const move = (id: string, delta: number) => {
    const index = orderIds.indexOf(id)
    if (index < 0) return
    const target = Math.max(0, Math.min(orderIds.length - 1, index + delta))
    if (index === target) return
    const next = [...orderIds]
    next.splice(index, 1)
    next.splice(target, 0, id)
    setOrderIds(next)
  }

  const saveOrder = async () => {
    if (mutation) return
    setMutation('order')
    try {
      // 后端要求提交当前全部相册且不重复的完整集合。
      const value = await adminApi.adminFetch<Album[]>('/api/albums/order', {
        method: 'POST',
        body: { albumIds: orderIds },
      })
      setAlbums(value)
      setOrderMode(false)
      notice.add({ title: '相册顺序已保存', color: 'success' })
    } catch (cause) {
      // 失败时保持排序模式，便于用户重试。
      notice.add({
        title: '顺序保存失败，请刷新后重试',
        description: getAdminApiErrorMessage(cause),
        color: 'error',
      })
    } finally {
      setMutation('')
    }
  }

  /* -------------------------------- 图片 -------------------------------- */

  const queueFile = (file: File) => {
    if (!selectedAlbum || locked || !ready) return
    if (!isAlbumUploadable(file.name)) {
      notice.add({ title: `不支持此文件：${file.name}`, color: 'warning' })
      return
    }
    // albumId 在入队时快照进每一项，之后切换相册不会改变目标。
    uploads.queue.enqueue([file], { id: selectedAlbum.id, name: selectedAlbum.name })
  }

  const setCoverFromPhoto = async () => {
    if (locked || selectedPhotos.length !== 1) return
    setMutation('cover')
    try {
      const cover = await adminApi.adminFetch<AlbumCover>(`/api/albums/${selectedId}/cover`, {
        method: 'PUT',
        body: { photoId: selectedPhotos[0] },
      })
      setAlbums((previous) => previous.map((album) => (
        album.id === selectedId ? { ...album, ...cover } : album
      )))
      notice.add({ title: '已设为相册封面', color: 'success' })
    } catch (cause) {
      notice.add({ title: '封面设置失败', description: getAdminApiErrorMessage(cause), color: 'error' })
    } finally {
      setMutation('')
    }
  }

  const deletePhotos = async () => {
    if (locked || !selectedPhotos.length) return
    const count = selectedPhotos.length
    setMutation('confirm-delete-photos')
    const ok = await notice.confirm(
      `永久删除选中的 ${count} 张图片？本地、S3 或 WebDAV 中的对应图片和缓存也会删除，无法撤销。`,
      true,
    )
    if (!ok) {
      setMutation('')
      return
    }
    setMutation('delete-photos')
    try {
      // 先拷副本，避免请求期间选择发生变化。
      const ids = [...selectedPhotos]
      const result = await adminApi.adminFetch<PhotoDeletionResult>('/api/photos/delete', {
        method: 'POST',
        body: { photoIds: ids },
      })
      // 重新拉整个详情：照片列表与相册计数都以服务端为准。
      await loadDetail(selectedId)
      notice.add({
        title: `已删除 ${result.deleted} 张图片`,
        description: result.cleanupPending
          ? `${result.cleanupPending} 个存储对象将在后台继续清理`
          : undefined,
        color: result.failures.length || result.cleanupPending ? 'warning' : 'success',
      })
    } catch (cause) {
      notice.add({
        title: '删除失败，请刷新确认',
        description: getAdminApiErrorMessage(cause),
        color: 'error',
      })
    } finally {
      setMutation('')
    }
  }

  /* ------------------------------- 派生数据 ----------------------------- */

  const filteredAlbums = useMemo(() => {
    if (orderMode) {
      return orderIds
        .map((id) => albums.find((album) => album.id === id))
        .filter((album): album is Album => Boolean(album))
    }
    const query = listState.query.trim().toLocaleLowerCase()
    if (!query) return albums
    return albums.filter((album) =>
      `${album.name} ${album.description || ''}`.toLocaleLowerCase().includes(query))
  }, [albums, listState.query, orderIds, orderMode])

  const pagedAlbums = filteredAlbums.slice(
    (listState.page - 1) * ALBUM_PAGE_SIZE,
    listState.page * ALBUM_PAGE_SIZE,
  )

  const filteredPhotos = useMemo(() => {
    const query = photoQuery.trim().toLocaleLowerCase()
    const result = photos.filter((photo) => {
      if (query && !photo.originalName.toLocaleLowerCase().includes(query)) return false
      // 格式是精确相等；后端的 format 只有 png/jpg/webp，jpeg 会存成 jpg。
      if (photoFormat !== 'all' && photo.format !== photoFormat) return false
      return true
    })
    if (photoSort === 'name') result.sort((a, b) => a.originalName.localeCompare(b.originalName))
    else if (photoSort === 'size') result.sort((a, b) => b.byteSize - a.byteSize)
    else result.sort((a, b) => b.createdAt - a.createdAt || b.id.localeCompare(a.id))
    return result
  }, [photos, photoFormat, photoQuery, photoSort])

  const pagePhotos = filteredPhotos.slice(
    (photoPage - 1) * PHOTO_PAGE_SIZE,
    photoPage * PHOTO_PAGE_SIZE,
  )
  const pageChecked = pagePhotos.length > 0
    && pagePhotos.every((photo) => selectedPhotos.includes(photo.id))
  const pagePartial = !pageChecked && pagePhotos.some((photo) => selectedPhotos.includes(photo.id))
  const exportUrl = `/api/albums/export?${new URLSearchParams({ albumIds: listState.selected.join(',') })}`

  /* -------------------------------- 列定义 ------------------------------ */

  const albumColumns: Column<Album>[] = orderMode
    ? [
      {
        key: 'order',
        title: '顺序',
        width: 210,
        render: (album, index) => (
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <span style={{ width: 20, color: 'var(--muted)' }}>{index + 1}</span>
            <Button
              variant="ghost" size="sm" isIconOnly
              aria-label={`上移${album.name}`}
              isDisabled={locked || index === 0}
              onPress={() => move(album.id, -1)}
            >
              <Icon name="arrow-up" size={14} />
            </Button>
            <Button
              variant="ghost" size="sm" isIconOnly
              aria-label={`下移${album.name}`}
              isDisabled={locked || index === albums.length - 1}
              onPress={() => move(album.id, 1)}
            >
              <Icon name="arrow-down" size={14} />
            </Button>
            <Button
              variant="ghost" size="sm"
              isDisabled={locked || index === 0}
              onPress={() => move(album.id, -albums.length)}
            >
              置顶
            </Button>
          </div>
        ),
      },
      { key: 'album', title: '相册', render: (album) => <AlbumCell album={album} /> },
      { key: 'photoCount', title: '图片', width: 90, render: (album) => album.photoCount },
    ]
    : [
      {
        key: 'album',
        title: '相册',
        render: (album) => <AlbumCell album={album} onOpen={() => navigateAlbum(album.id)} />,
      },
      { key: 'photoCount', title: '图片', width: 90, render: (album) => album.photoCount },
      {
        key: 'actions',
        title: '操作',
        width: 200,
        render: (album) => (
          <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
            <LinkButton label="管理图片" onPress={() => navigateAlbum(album.id)} />
            <LinkButton label="资料" onPress={() => navigateAlbum(album.id, 'details')} />
            <LinkButton label="下载" onPress={() => navigateAlbum(album.id, 'downloads')} />
          </div>
        ),
      },
    ]

  const photoTableColumns: Column<Photo>[] = [
    {
      key: 'photo',
      title: '图片',
      width: 90,
      render: (photo) => (
        <button
          type="button"
          className="admin-photo-preview"
          aria-label={`预览图片：${photo.originalName}`}
          onClick={() => setPreview(photo)}
          // 表格里的热区只包住 56×56 的缩略图，不要撑满整个单元格。
          style={{ width: 'auto' }}
        >
          <img
            src={`/api/photos/${encodeURIComponent(photo.id)}/thumbnail?v=grid2`}
            alt=""
            loading="lazy"
            style={{ width: 56, height: 56, objectFit: 'cover', borderRadius: 6 }}
          />
        </button>
      ),
    },
    { key: 'name', title: '文件名', render: (photo) => photo.originalName },
    { key: 'format', title: '格式', width: 90, render: (photo) => photo.format.toUpperCase() },
    {
      key: 'dimensions',
      title: '尺寸',
      width: 140,
      render: (photo) => `${photo.width} × ${photo.height}`,
    },
    { key: 'size', title: '大小', width: 110, render: (photo) => adminBytes(photo.byteSize) },
  ]

  /* -------------------------------- 渲染 -------------------------------- */

  return (
    <div>
      {error ? (
        <div style={{ marginBottom: 20 }}>
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Description>
                <span className="preserve-newlines">{error}</span>
              </Alert.Description>
            </Alert.Content>
          </Alert>
        </div>
      ) : null}

      {/* ============================== 列表视图 ============================== */}
      {!selectedId ? (
        <>
          <PageHeader
            title="相册管理"
            description="先创建相册，再添加图片。点击相册名称进入管理。"
            actions={
              <>
                <Button
                  variant="secondary"
                  isDisabled={loading || locked || orderMode || albums.length < 2}
                  onPress={() => {
                    setOrderIds(albums.map((album) => album.id))
                    setOrderMode(true)
                  }}
                >
                  <Icon name="arrows-sort" size={16} />
                  调整顺序
                </Button>
                <Button
                  variant="primary"
                  isDisabled={locked || orderMode}
                  onPress={() => setCreateOpen(true)}
                >
                  <Icon name="plus" size={16} />
                  新建相册
                </Button>
              </>
            }
          />

          <Card>
            <Card.Content>
              <div className="admin-toolbar" style={{ marginBottom: 16 }}>
                {orderMode ? (
                  <div>
                    <strong>调整首页展示顺序</strong>
                    <p className="admin-help" style={{ marginTop: 4 }}>
                      上移、下移或置顶，最后统一保存。
                    </p>
                  </div>
                ) : (
                  <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                    <TextField
                      value={listState.query}
                      className="w-[260px] max-w-[70vw]"
                      onChange={(query: string) =>
                        listView.update((previous) => ({ ...previous, query, page: 1 }))}
                    >
                      <Input placeholder="搜索名称或简介" aria-label="搜索相册" />
                    </TextField>
                    <span className="admin-help">{filteredAlbums.length} 个相册</span>
                  </div>
                )}

                <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                  {orderMode ? (
                    <>
                      <Button variant="ghost" isDisabled={locked} onPress={() => setOrderMode(false)}>
                        取消
                      </Button>
                      <Button
                        variant="primary"
                        isDisabled={!orderDirty}
                        isPending={mutation === 'order'}
                        onPress={() => void saveOrder()}
                      >
                        保存顺序
                      </Button>
                    </>
                  ) : (
                    <>
                      <Button variant="secondary" isDisabled={loading} onPress={() => void loadAlbums()}>
                        <Icon name="refresh" size={16} />
                        刷新
                      </Button>
                      {listState.selected.length ? (
                        <>
                          <Button variant="secondary" onPress={() => setExportOpen(true)}>
                            导出原始文件（{listState.selected.length}）
                          </Button>
                          <Button
                            variant="ghost"
                            onPress={() =>
                              listView.update((previous) => ({ ...previous, selected: [] }))}
                          >
                            取消选择
                          </Button>
                        </>
                      ) : null}
                    </>
                  )}
                </div>
              </div>

              <DataTable
                columns={albumColumns}
                rows={pagedAlbums}
                rowKey={(album) => album.id}
                loading={loading}
                emptyText="还没有相册"
                minWidth={700}
                selection={orderMode ? undefined : {
                  selectedKeys: listState.selected,
                  onChange: (keys) =>
                    listView.update((previous) => ({ ...previous, selected: keys })),
                }}
              />

              {!orderMode ? (
                <div
                  style={{
                    display: 'flex', flexWrap: 'wrap', justifyContent: 'space-between',
                    alignItems: 'center', gap: 16, marginTop: 16,
                  }}
                >
                  <span className="admin-help">
                    共 {filteredAlbums.length} 个相册 · 每页 {ALBUM_PAGE_SIZE} 个
                  </span>
                  <Pager
                    page={listState.page}
                    pageSize={ALBUM_PAGE_SIZE}
                    total={filteredAlbums.length}
                    onChange={(page) => listView.update((previous) => ({ ...previous, page }))}
                  />
                </div>
              ) : null}
            </Card.Content>
          </Card>
        </>
      ) : null}

      {/* ============================= 工作区视图 ============================= */}
      {selectedId ? (
        <>
          <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 12 }}>
            <LinkButton label="← 全部相册" onPress={() => navigateAlbum()} />
            <span className="admin-help">/ {selectedAlbum?.name || '相册'}</span>
          </div>

          <PageHeader
            title={selectedAlbum?.name || '加载相册'}
            description={`${selectedAlbum?.photoCount || 0} 张图片 · 在同一个工作区完成图片、资料和下载管理`}
            actions={
              <>
                <Button
                  variant="secondary"
                  onPress={() =>
                    window.open(`/albums/${encodeURIComponent(selectedId)}`, '_blank', 'noopener')}
                >
                  查看公开页面
                  <Icon name="external-link" size={16} />
                </Button>
                <Button
                  variant="secondary"
                  isDisabled={loading || locked}
                  onPress={() => void loadDetail(selectedId)}
                >
                  <Icon name="refresh" size={16} />
                  刷新
                </Button>
                <UploadButton
                  disabled={locked || !ready}
                  label="上传图片"
                  primary
                  onFiles={(files) => files.forEach(queueFile)}
                />
              </>
            }
          />

          {detailError ? (
            <div style={{ marginBottom: 20 }}>
              <Alert status="danger">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Description>
                    <span className="preserve-newlines">{detailError}</span>
                  </Alert.Description>
                </Alert.Content>
              </Alert>
            </div>
          ) : null}

          {!selectedAlbum && !loading && !detailLoading && !detailError ? (
            <div style={{ marginBottom: 20 }}>
              <Alert status="warning">
                <Alert.Indicator />
                <Alert.Content>
                  <Alert.Description>相册不存在，请返回列表重新选择。</Alert.Description>
                </Alert.Content>
              </Alert>
            </div>
          ) : null}

          {selectedAlbum && ready ? (
            <Tabs selectedKey={tab} onSelectionChange={(key) => navigateAlbum(selectedId, String(key))}>
              <Tabs.List>
                <Tabs.Tab id="photos">图片管理（{photos.length}）</Tabs.Tab>
                <Tabs.Tab id="details">相册资料</Tabs.Tab>
                <Tabs.Tab id="downloads">公开下载</Tabs.Tab>
              </Tabs.List>
            </Tabs>
          ) : null}

          {/* ---------------------------- 图片管理页签 ---------------------------- */}
          {selectedAlbum && ready && tab === 'photos' ? (
            <div className="admin-stack" style={{ marginTop: 20 }}>
              {albumUploads > 0 ? (
                <Alert status="accent">
                  <Alert.Indicator />
                  <Alert.Content>
                    <Alert.Description>
                      <span style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                        {albumUploads} 张图片等待上传或处理中；可切换页面，成功入库后自动显示。
                        <Button variant="ghost" size="sm" onPress={() => uploads.setOpen(true)}>
                          查看队列
                        </Button>
                      </span>
                    </Alert.Description>
                  </Alert.Content>
                </Alert>
              ) : null}

              {!photos.length && !detailError ? (
                <Card>
                  <Card.Content>
                    <UploadDropZone
                      disabled={locked}
                      label="拖入图片，或点击开始上传"
                      hint="选择后自动上传，7 并发；入库时自动生成三层预览。"
                      onFiles={(files) => files.forEach(queueFile)}
                    />
                  </Card.Content>
                </Card>
              ) : (
                <Card>
                  <Card.Content>
                    <div style={{ marginBottom: 16 }}>
                      <UploadDropZone
                        compact
                        disabled={locked}
                        label="拖入更多图片，或点击选择 · 自动加入上传队列"
                        onFiles={(files) => files.forEach(queueFile)}
                      />
                    </div>

                    <div className="admin-toolbar" style={{ marginBottom: 16 }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                        <TextField
                          value={photoQuery}
                          className="w-[230px]"
                          onChange={(value: string) => { setPhotoQuery(value); setPhotoPage(1) }}
                        >
                          <Input placeholder="搜索文件名" aria-label="搜索图片" />
                        </TextField>
                        <NativeSelect
                          ariaLabel="筛选图片格式"
                          value={photoFormat}
                          options={PHOTO_FORMAT_OPTIONS}
                          onChange={(value) => {
                            setPhotoFormat(value as typeof photoFormat)
                            setPhotoPage(1)
                          }}
                        />
                        <NativeSelect
                          ariaLabel="图片排序"
                          value={photoSort}
                          options={PHOTO_SORT_OPTIONS}
                          onChange={(value) => setPhotoSort(value as typeof photoSort)}
                        />
                      </div>
                      <div style={{ display: 'flex', gap: 16 }}>
                        <RadioGroup
                          value={photoView.value}
                          onChange={(value) => photoView.set(value as 'grid' | 'table')}
                          aria-label="图片视图"
                          style={{ display: 'flex', gap: 12 }}
                        >
                          <Radio value="grid">网格</Radio>
                          <Radio value="table">列表</Radio>
                        </RadioGroup>
                      </div>
                    </div>

                    <div className="admin-selection-bar" style={{ marginBottom: 16 }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                        <Checkbox
                          aria-label="本页全选"
                          isSelected={pageChecked}
                          isIndeterminate={pagePartial}
                          isDisabled={locked || !pagePhotos.length}
                          onChange={(checked: boolean) => setSelectedPhotos((previous) =>
                            toggleVisibleSelection(previous, pagePhotos.map((photo) => photo.id), checked))}
                        >
                          本页全选
                        </Checkbox>
                        <span className="admin-help">
                          已选 {selectedPhotos.length} / {photos.length}
                        </span>
                        <Button
                          variant="ghost"
                          size="sm"
                          isDisabled={locked || !filteredPhotos.length}
                          onPress={() => setSelectedPhotos((previous) =>
                            toggleVisibleSelection(previous, filteredPhotos.map((photo) => photo.id), true))}
                        >
                          选择全部筛选结果（{filteredPhotos.length}）
                        </Button>
                        {selectedPhotos.length ? (
                          <Button variant="ghost" size="sm" onPress={() => setSelectedPhotos([])}>
                            取消选择
                          </Button>
                        ) : null}
                      </div>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <Button
                          variant="secondary"
                          size="sm"
                          isDisabled={locked || selectedPhotos.length !== 1}
                          onPress={() => void setCoverFromPhoto()}
                        >
                          设为封面
                        </Button>
                        <Button
                          variant="danger"
                          size="sm"
                          isDisabled={!selectedPhotos.length || (locked && mutation !== 'delete-photos')}
                          isPending={mutation === 'delete-photos'}
                          onPress={() => void deletePhotos()}
                        >
                          删除（{selectedPhotos.length}）
                        </Button>
                      </div>
                    </div>

                    {photoView.value === 'grid' ? (
                      pagePhotos.length ? (
                        <div className="admin-photo-grid">
                          {pagePhotos.map((photo) => {
                            const selected = selectedPhotos.includes(photo.id)
                            const toggle = () => setSelectedPhotos((previous) =>
                              toggleVisibleSelection(previous, [photo.id], !selected))
                            return (
                              <article key={photo.id} className="admin-photo-tile" data-selected={selected}>
                                <button
                                  type="button"
                                  className="admin-photo-preview"
                                  aria-label={`预览图片：${photo.originalName}`}
                                  onClick={() => setPreview(photo)}
                                >
                                  <img
                                    src={`/api/photos/${encodeURIComponent(photo.id)}/thumbnail?v=grid2`}
                                    alt={photo.originalName}
                                    loading="lazy"
                                  />
                                </button>
                                <span className="admin-photo-check">
                                  <Checkbox
                                    aria-label={`选择图片：${photo.originalName}`}
                                    isSelected={selected}
                                    isDisabled={locked}
                                    onChange={toggle}
                                  />
                                </span>
                                {selectedAlbum.coverPhotoId === photo.id ? (
                                  <span className="admin-cover-flag">
                                    <Chip color="accent" variant="primary" size="sm">封面</Chip>
                                  </span>
                                ) : null}
                                <button
                                  type="button"
                                  className="admin-photo-caption"
                                  title={photo.originalName}
                                  disabled={locked}
                                  onClick={toggle}
                                >
                                  <span>{photo.originalName}</span>
                                  <small>
                                    {photo.format.toUpperCase()} · {adminBytes(photo.byteSize)}
                                  </small>
                                </button>
                              </article>
                            )
                          })}
                        </div>
                      ) : (
                        <p className="admin-help" style={{ padding: '32px 0', textAlign: 'center' }}>
                          没有符合条件的图片
                        </p>
                      )
                    ) : (
                      <DataTable
                        columns={photoTableColumns}
                        rows={pagePhotos}
                        rowKey={(photo) => photo.id}
                        emptyText="没有符合条件的图片"
                        minWidth={700}
                        selection={{
                          selectedKeys: selectedPhotos,
                          onChange: setSelectedPhotos,
                          disabled: locked,
                        }}
                      />
                    )}

                    <div
                      style={{
                        display: 'flex', flexWrap: 'wrap', justifyContent: 'space-between',
                        alignItems: 'center', gap: 16, marginTop: 20,
                      }}
                    >
                      <span className="admin-help">
                        {filteredPhotos.length} 张符合条件 · 每页 {PHOTO_PAGE_SIZE} 张
                      </span>
                      <Pager
                        page={photoPage}
                        pageSize={PHOTO_PAGE_SIZE}
                        total={filteredPhotos.length}
                        onChange={setPhotoPage}
                      />
                    </div>
                  </Card.Content>
                </Card>
              )}
            </div>
          ) : null}

          {/* ---------------------------- 相册资料页签 ---------------------------- */}
          {selectedAlbum && ready && tab === 'details' ? (
            <div className="admin-stack" style={{ marginTop: 20 }}>
              <Card>
                <Card.Header>
                  <Card.Title>名称、简介与展示日期</Card.Title>
                </Card.Header>
                <Card.Content>
                  <form
                    onSubmit={(event) => { event.preventDefault(); void save() }}
                    style={{ display: 'flex', flexDirection: 'column', gap: 18 }}
                  >
                    <div className="admin-form-grid">
                      <TextField
                        value={draft.name}
                        isDisabled={locked}
                        onChange={(name: string) => setDraft((previous) => ({ ...previous, name }))}
                      >
                        <Label>相册名称</Label>
                        <Input maxLength={100} />
                      </TextField>
                      <DateField
                        label="展示创建日期"
                        value={draft.displayCreatedDate}
                        disabled={locked}
                        placeholder="自动日期"
                        onChange={(value) =>
                          setDraft((previous) => ({ ...previous, displayCreatedDate: value }))}
                      />
                    </div>

                    <TextField
                      value={draft.description}
                      isDisabled={locked}
                      onChange={(description: string) =>
                        setDraft((previous) => ({ ...previous, description }))}
                    >
                      <Label>相册简介</Label>
                      <TextArea rows={3} maxLength={1000} placeholder="显示在公开相册页面的介绍" />
                    </TextField>

                    <div className="admin-form-grid">
                      <DateField
                        label="图片开始日期"
                        value={draft.photoDateStart}
                        disabled={locked}
                        placeholder="自动日期"
                        onChange={(value) =>
                          setDraft((previous) => ({ ...previous, photoDateStart: value }))}
                      />
                      <DateField
                        label="图片结束日期"
                        value={draft.photoDateEnd}
                        disabled={locked}
                        placeholder="自动日期"
                        onChange={(value) =>
                          setDraft((previous) => ({ ...previous, photoDateEnd: value }))}
                      />
                    </div>
                    <p className="admin-help">留空使用真实创建日期。</p>

                    <div>
                      <Button
                        variant="ghost"
                        size="sm"
                        isDisabled={locked}
                        onPress={() => setDraft((previous) => ({
                          ...previous,
                          displayCreatedDate: null,
                          photoDateStart: null,
                          photoDateEnd: null,
                        }))}
                      >
                        恢复自动日期（保存后生效）
                      </Button>
                    </div>

                    {formError ? (
                      <Alert status="danger">
                        <Alert.Content>
                          <Alert.Description>
                            <span className="preserve-newlines">{formError}</span>
                          </Alert.Description>
                        </Alert.Content>
                      </Alert>
                    ) : null}

                    <div className="admin-save-bar">
                      <span className={dirty ? 'admin-unsaved' : 'admin-help'}>
                        {dirty ? '有未保存的修改' : '所有资料已保存'}
                      </span>
                      <div style={{ display: 'flex', gap: 8 }}>
                        <Button
                          variant="ghost"
                          isDisabled={!dirty || locked}
                          onPress={() => applyDraft(selectedAlbum)}
                        >
                          放弃修改
                        </Button>
                        <Button
                          type="submit"
                          variant="primary"
                          isDisabled={!dirty}
                          isPending={mutation === 'save'}
                        >
                          保存相册资料
                        </Button>
                      </div>
                    </div>
                  </form>
                </Card.Content>
              </Card>

              <AlbumCoverEditor
                key={selectedId}
                album={selectedAlbum}
                photos={photos}
                disabled={locked}
                onSaved={(albumId, cover) => setAlbums((previous) => previous.map((album) => (
                  album.id === albumId ? { ...album, ...cover } : album
                )))}
                onBusy={setCoverBusy}
              />

              <Card>
                <Card.Content>
                  <div className="admin-toolbar">
                    <div>
                      <strong>删除相册</strong>
                      <p className="admin-help" style={{ marginTop: 4 }}>
                        将删除相册及其中全部图片、封面和压缩包，无法撤销。
                      </p>
                    </div>
                    <Button
                      variant="danger"
                      isDisabled={locked || dirty || downloadDirty}
                      isPending={mutation === 'delete-album' || mutation === 'confirm-delete-album'}
                      onPress={() => void removeAlbum()}
                    >
                      删除此相册
                    </Button>
                  </div>
                </Card.Content>
              </Card>
            </div>
          ) : null}

          {/* ---------------------------- 公开下载页签 ---------------------------- */}
          {selectedAlbum && ready && tab === 'downloads' ? (
            <div style={{ marginTop: 20 }}>
              <DownloadManager
                key={selectedId}
                albumId={selectedId}
                embedded
                onDirty={setDownloadDirty}
                onBusy={setDownloadBusy}
              />
            </div>
          ) : null}
        </>
      ) : null}

      {/* ============================== 新建相册 ============================== */}
      <Modal.Root
        isOpen={createOpen}
        onOpenChange={(next: boolean) => { if (!next && !locked) setCreateOpen(false) }}
      >
        <Modal.Backdrop isDismissable={!locked}>
          <Modal.Container>
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>新建相册</Modal.Heading>
              </Modal.Header>
              <Modal.Body>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
                  <TextField
                    value={newAlbum.name}
                    isDisabled={locked}
                    onChange={(name: string) => setNewAlbum((previous) => ({ ...previous, name }))}
                  >
                    <Label>相册名称</Label>
                    <Input maxLength={100} placeholder="例如：2026 夏日旅行" />
                  </TextField>
                  <TextField
                    value={newAlbum.description}
                    isDisabled={locked}
                    onChange={(description: string) =>
                      setNewAlbum((previous) => ({ ...previous, description }))}
                  >
                    <Label>简介（选填）</Label>
                    <TextArea rows={3} maxLength={1000} />
                  </TextField>
                  {createError ? <p className="admin-field-error">{createError}</p> : null}
                </div>
              </Modal.Body>
              <Modal.Footer>
                <Button variant="ghost" isDisabled={locked} onPress={() => setCreateOpen(false)}>
                  取消
                </Button>
                <Button variant="primary" isPending={mutation === 'create'} onPress={() => void create()}>
                  创建并上传图片
                </Button>
              </Modal.Footer>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal.Root>

      {/* ============================ 导出原始文件 ============================ */}
      <Modal.Root isOpen={exportOpen} onOpenChange={setExportOpen}>
        <Modal.Backdrop>
          <Modal.Container>
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>管理员导出原始文件</Modal.Heading>
              </Modal.Header>
              <Modal.Body>
                <p style={{ margin: 0 }}>
                  导出选中的 {listState.selected.length} 个相册，保持入库文件格式，不改变公开下载设置。
                </p>
                <p className="admin-help" style={{ marginTop: 12 }}>
                  一个相册生成一个 ZIP；多个相册生成一个外层 ZIP，其中每个相册各一个 ZIP。大相册需要等待服务器打包。
                </p>
              </Modal.Body>
              <Modal.Footer>
                <Button variant="ghost" onPress={() => setExportOpen(false)}>取消</Button>
                <Button
                  variant="primary"
                  isDisabled={!listState.selected.length}
                  onPress={() => {
                    // 用原生导航触发下载，不走 fetch，因此不受 CSRF 头限制。
                    window.location.href = exportUrl
                    setExportOpen(false)
                  }}
                >
                  下载原始文件
                </Button>
              </Modal.Footer>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal.Root>

      {/* 图片预览浮层。挂在页面根部，网格与表格两处缩略图共用同一个实例。 */}
      <Lightbox photo={preview} onClose={() => setPreview(null)} />
    </div>
  )
}

/* =============================== 局部组件 =============================== */

/** 相册单元格：封面 + 名称 + 简介。 */
function AlbumCell({ album, onOpen }: { album: Album, onOpen?: () => void }) {
  return (
    <div className="admin-album-cell">
      {album.coverUrl ? (
        <img src={album.coverUrl} alt="" loading="lazy" />
      ) : (
        <span className="admin-album-placeholder"><Icon name="album" size={24} /></span>
      )}
      <div style={{ minWidth: 0 }}>
        {onOpen ? (
          <button
            type="button"
            onClick={onOpen}
            style={{
              background: 'none', border: 0, padding: 0, cursor: 'pointer',
              color: 'var(--link)', fontSize: 14, fontWeight: 500, textAlign: 'left',
            }}
          >
            {album.name}
          </button>
        ) : (
          <span style={{ fontSize: 14, fontWeight: 500 }}>{album.name}</span>
        )}
        <div className="admin-album-description" title={album.description || ''}>
          {album.description || '暂无简介'}
        </div>
      </div>
    </div>
  )
}

/** 文本样式的行内操作按钮。 */
function LinkButton({ label, onPress }: { label: string, onPress: () => void }) {
  return (
    <button
      type="button"
      onClick={onPress}
      style={{
        background: 'none', border: 0, padding: 0, cursor: 'pointer',
        color: 'var(--link)', fontSize: 14,
      }}
    >
      {label}
    </button>
  )
}

/**
 * 日期字段。用原生 date 输入而不是 RAC 的 DatePicker：
 * 后端契约就是 `YYYY-MM-DD` 字符串，原生输入的 value 格式与之一致，
 * 省掉一层日期对象转换，也不会引入时区偏移问题。
 */
function DateField({ label, value, onChange, disabled, placeholder }: {
  label: string
  value: string | null
  onChange: (value: string | null) => void
  disabled?: boolean
  placeholder?: string
}) {
  return (
    <TextField
      value={value ?? ''}
      isDisabled={disabled}
      onChange={(next: string) => onChange(next || null)}
    >
      <Label>{label}</Label>
      <Input type="date" placeholder={placeholder} />
    </TextField>
  )
}

/** 原生下拉，用于图片格式/排序这类低交互成本的筛选。 */
function NativeSelect({ ariaLabel, value, options, onChange }: {
  ariaLabel: string
  value: string
  options: Array<{ value: string, label: string }>
  onChange: (value: string) => void
}) {
  return (
    <select
      aria-label={ariaLabel}
      value={value}
      onChange={(event) => onChange(event.target.value)}
      style={{
        height: 36,
        borderRadius: 8,
        border: '1px solid var(--border)',
        background: 'var(--surface)',
        color: 'var(--foreground)',
        padding: '0 8px',
        fontSize: 14,
      }}
    >
      {options.map((option) => (
        <option key={option.value} value={option.value}>{option.label}</option>
      ))}
    </select>
  )
}

/** 上传按钮：隐藏的 file input + HeroUI Button 触发。 */
function UploadButton({ disabled, onFiles, label, primary = false }: {
  disabled: boolean
  onFiles: (files: File[]) => void
  label: string
  primary?: boolean
}) {
  const input = useRef<HTMLInputElement>(null)
  return (
    <>
      <input
        ref={input}
        type="file"
        accept={ALBUM_UPLOAD_ACCEPT}
        multiple
        style={{ display: 'none' }}
        onChange={(event) => {
          const files = Array.from(event.target.files ?? [])
          event.target.value = ''
          if (files.length) onFiles(files)
        }}
      />
      <Button
        variant={primary ? 'primary' : 'secondary'}
        isDisabled={disabled}
        onPress={() => input.current?.click()}
      >
        <Icon name="upload" size={16} />
        {label}
      </Button>
    </>
  )
}

/** 拖放 + 点击选择的上传区。 */
function UploadDropZone({ disabled, label, hint, compact = false, onFiles }: {
  disabled: boolean
  label: string
  hint?: string
  compact?: boolean
  onFiles: (files: File[]) => void
}) {
  const [over, setOver] = useState(false)
  const input = useRef<HTMLInputElement>(null)
  return (
    <>
      <input
        ref={input}
        type="file"
        accept={ALBUM_UPLOAD_ACCEPT}
        multiple
        style={{ display: 'none' }}
        onChange={(event) => {
          const files = Array.from(event.target.files ?? [])
          event.target.value = ''
          if (files.length) onFiles(files)
        }}
      />
      <div
        role="button"
        tabIndex={disabled ? -1 : 0}
        onClick={() => { if (!disabled) input.current?.click() }}
        onKeyDown={(event) => {
          if (!disabled && (event.key === 'Enter' || event.key === ' ')) input.current?.click()
        }}
        onDragOver={(event) => { if (!disabled) { event.preventDefault(); setOver(true) } }}
        onDragLeave={() => setOver(false)}
        onDrop={(event) => {
          if (disabled) return
          event.preventDefault()
          setOver(false)
          const files = Array.from(event.dataTransfer.files ?? [])
          if (files.length) onFiles(files)
        }}
        style={{
          border: `1px dashed ${over ? 'var(--accent)' : 'var(--border)'}`,
          background: over ? 'color-mix(in oklab, var(--accent) 6%, var(--surface))' : 'transparent',
          borderRadius: 10,
          padding: compact ? 12 : 32,
          textAlign: 'center',
          cursor: disabled ? 'not-allowed' : 'pointer',
          opacity: disabled ? 0.6 : 1,
        }}
      >
        <div style={{ fontSize: compact ? 13 : 15, color: 'var(--foreground)' }}>{label}</div>
        {hint ? <div className="admin-help" style={{ marginTop: 6 }}>{hint}</div> : null}
      </div>
    </>
  )
}
