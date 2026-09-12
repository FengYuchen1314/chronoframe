import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { Alert, Button, Card } from '@heroui/react'
import { getAdminApiErrorMessage, adminApi } from '../lib/api'
import type { Album, StorageBackend, StorageSettings } from '../lib/types'
import { Icon } from '../lib/icons'
import PageHeader from '../components/PageHeader'
import DataTable from '../components/DataTable'
import type { Column } from '../components/DataTable'
import Pager from '../components/Pager'
import { useDocumentTitle } from '../lib/hooks'

const storageLabels: Record<StorageBackend, string> = {
  local: '本地存储',
  webdav: 'WebDAV',
  s3: 'S3 对象存储',
}

const PAGE_SIZE = 8

export default function Overview() {
  useDocumentTitle('概览')
  const navigate = useNavigate()
  const [albums, setAlbums] = useState<Album[]>([])
  const [storage, setStorage] = useState<StorageSettings | null>(null)
  const [loading, setLoading] = useState(false)
  const [loadError, setLoadError] = useState('')
  const [page, setPage] = useState(1)

  // 重入保护用 ref 而不是 state：把 loading 放进 useCallback 的依赖里会让
  // 「请求完成 → setLoading(false) → 回调换身份 → effect 重跑 → 再发一次请求」
  // 形成无限循环，表现就是页面持续闪烁。依赖保持为空，effect 只跑一次。
  const inFlight = useRef(false)

  const refreshAll = useCallback(async () => {
    if (inFlight.current) return
    inFlight.current = true
    setLoading(true)
    setLoadError('')
    try {
      // 两个接口互不阻塞：一个失败时另一个的数据仍然展示（对应现网的 allSettled 语义）。
      const [albumsResult, storageResult] = await Promise.allSettled([
        adminApi.adminFetch<Album[]>('/api/albums'),
        adminApi.adminFetch<StorageSettings>('/api/settings/storage'),
      ])

      const failures: string[] = []

      if (albumsResult.status === 'fulfilled') setAlbums(albumsResult.value)
      else failures.push(`相册：${getAdminApiErrorMessage(albumsResult.reason)}`)

      if (storageResult.status === 'fulfilled') setStorage(storageResult.value)
      else failures.push(`存储：${getAdminApiErrorMessage(storageResult.reason)}`)

      setLoadError(failures.join('\n'))
    } catch (cause) {
      setLoadError(getAdminApiErrorMessage(cause))
    } finally {
      setLoading(false)
      inFlight.current = false
    }
  }, [])

  useEffect(() => { void refreshAll() }, [refreshAll])

  const photoCount = albums.reduce((total, album) => total + album.photoCount, 0)
  const pageAlbums = albums.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE)

  const columns: Column<Album>[] = [
    {
      key: 'name',
      title: '相册名称',
      render: (album) => (
        <Link
          to={`/albums?album=${encodeURIComponent(album.id)}`}
          style={{ color: 'var(--link)', fontWeight: 500 }}
        >
          {album.name}
        </Link>
      ),
    },
    { key: 'photoCount', title: '图片数量', width: 120, render: (album) => album.photoCount },
    {
      key: 'description',
      title: '简介',
      render: (album) => (
        <span
          style={{
            display: 'block',
            maxWidth: 420,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            color: 'var(--muted)',
          }}
        >
          {album.description || '暂无简介'}
        </span>
      ),
    },
    {
      key: 'actions',
      title: '操作',
      width: 170,
      render: (album) => (
        <div style={{ display: 'flex', gap: 12 }}>
          <Link to={`/albums?album=${encodeURIComponent(album.id)}`} style={{ color: 'var(--link)' }}>
            管理图片
          </Link>
          <Link
            to={`/albums?album=${encodeURIComponent(album.id)}&tab=downloads`}
            style={{ color: 'var(--link)' }}
          >
            下载设置
          </Link>
        </div>
      ),
    },
  ]

  return (
    <div>
      <PageHeader
        title="概览"
        description="查看相册、图片和存储状态，快速进入日常管理。"
        actions={
          <>
            <Button variant="secondary" isDisabled={loading} onPress={() => void refreshAll()}>
              <Icon name="refresh" size={16} />
              刷新
            </Button>
            <Button variant="primary" onPress={() => navigate('/albums')}>
              <Icon name="album" size={16} />
              管理相册
            </Button>
          </>
        }
      />

      <div className="admin-stack">
        {loadError ? (
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Description>
                <span className="preserve-newlines">{loadError}</span>
              </Alert.Description>
            </Alert.Content>
          </Alert>
        ) : null}

        <Card>
          <Card.Header>
            <Card.Title>从这里开始</Card.Title>
          </Card.Header>
          <Card.Content>
            <div className="admin-quick-actions">
              <Link to="/albums">
                <Icon name="photo-plus" size={28} />
                <strong>添加与整理图片</strong>
                <span>创建相册、上传、批量选图</span>
              </Link>
              <Link to="/downloads">
                <Icon name="file-zip" size={28} />
                <strong>管理公开下载</strong>
                <span>查看 ZIP 状态、批量设置</span>
              </Link>
              <Link to="/tasks">
                <Icon name="activity" size={28} />
                <strong>查看后台任务</strong>
                <span>生成进度、异常与待确认项</span>
              </Link>
            </div>
          </Card.Content>
        </Card>

        <div className="grid gap-4 sm:grid-cols-3">
          <StatCard label="相册总数" value={albums.length} />
          <StatCard label="图片总数" value={photoCount} />
          <StatCard label="当前图片存储" value={storage ? storageLabels[storage.backend] : '—'} />
        </div>

        <Card>
          <Card.Header>
            <Card.Title>相册</Card.Title>
          </Card.Header>
          <Card.Content>
            <DataTable
              columns={columns}
              rows={pageAlbums}
              rowKey={(album) => album.id}
              loading={loading}
              emptyText="还没有相册，先去创建一个吧"
              minWidth={600}
            />
            <div
              style={{
                display: 'flex',
                flexWrap: 'wrap',
                justifyContent: 'space-between',
                alignItems: 'center',
                gap: 16,
                marginTop: 16,
              }}
            >
              <span className="admin-help">
                共 {albums.length} 个相册 · 每页 {PAGE_SIZE} 个
              </span>
              <Pager
                page={page}
                pageSize={PAGE_SIZE}
                total={albums.length}
                onChange={setPage}
                disabled={loading}
              />
            </div>
          </Card.Content>
        </Card>

        <Card>
          <Card.Header>
            <Card.Title>常用操作</Card.Title>
          </Card.Header>
          <Card.Content>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 16 }}>
              <Button variant="secondary" onPress={() => navigate('/downloads')}>
                管理本地 ZIP
              </Button>
              <Button variant="secondary" onPress={() => navigate('/settings/storage')}>
                存储与缓存维护
              </Button>
              <Button variant="secondary" onPress={() => navigate('/settings/general')}>
                修改网站信息
              </Button>
            </div>
            <p className="admin-help" style={{ marginTop: 16 }}>
              图片原件使用当前存储；三层浏览缓存及相册下载 ZIP 始终存放在服务器本地数据目录。
            </p>
          </Card.Content>
        </Card>
      </div>
    </div>
  )
}

function StatCard({ label, value }: { label: string; value: number | string }) {
  return (
    <div
      style={{
        border: '1px solid var(--border)',
        borderRadius: 10,
        background: 'var(--surface)',
        padding: 18,
      }}
    >
      <div style={{ fontSize: 13, color: 'var(--muted)' }}>{label}</div>
      <div
        style={{
          marginTop: 6,
          fontSize: typeof value === 'string' ? 22 : 26,
          fontWeight: 600,
          color: 'var(--foreground)',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {value}
      </div>
    </div>
  )
}
