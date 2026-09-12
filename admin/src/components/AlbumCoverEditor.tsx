import { useRef, useState } from 'react'
import { Alert, Button, Card, Chip, Input, Modal, TextField } from '@heroui/react'
import { adminApi, getAdminApiErrorMessage } from '../lib/api'
import { isCoverUploadable } from '../lib/albums'
import { notice } from '../lib/notice'
import { Icon } from '../lib/icons'
import type { Album, AlbumCover, Photo } from '../lib/types'
import Pager from './Pager'

const PICKER_PAGE_SIZE = 24

const sourceLabel: Record<AlbumCover['coverSource'], string> = {
  upload: '单独上传',
  photo: '相册选图',
  auto: '自动封面',
}

/**
 * 相册封面编辑器。对应 Vue 侧的 app/components/dashboard/AlbumCoverEditor.vue。
 *
 * 关键约束：封面 URL 必须使用接口返回值。单独上传的封面每次替换都会换一个新的
 * UUID version，旧 URL 立刻 404——这是后端用来让 immutable 缓存失效的机制，
 * 自己拼接或缓存 URL 会拿到已经失效的地址。
 */
export default function AlbumCoverEditor({
  album,
  photos,
  disabled,
  onSaved,
  onBusy,
}: {
  album: Album
  photos: Photo[]
  disabled: boolean
  onSaved: (albumId: string, cover: AlbumCover) => void
  onBusy: (busy: boolean) => void
}) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [pickerOpen, setPickerOpen] = useState(false)
  const [selectedId, setSelectedId] = useState('')
  const [query, setQuery] = useState('')
  const [page, setPage] = useState(1)
  const fileInput = useRef<HTMLInputElement>(null)

  const locked = disabled || busy

  const filtered = photos.filter((photo) =>
    photo.originalName.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
  const paged = filtered.slice((page - 1) * PICKER_PAGE_SIZE, page * PICKER_PAGE_SIZE)
  const selectedPhoto = photos.find((photo) => photo.id === selectedId)

  const save = async (method: 'PUT' | 'POST' | 'DELETE', body?: unknown) => {
    if (locked) return
    setBusy(true)
    onBusy(true)
    setError('')
    try {
      const cover = await adminApi.adminFetch<AlbumCover>(
        `/api/albums/${encodeURIComponent(album.id)}/cover`,
        { method, body },
      )
      onSaved(album.id, cover)
      setPickerOpen(false)
      notice.add({
        title: method === 'DELETE' ? '已恢复自动封面' : '相册封面已更新',
        color: 'success',
      })
    } catch (cause) {
      const message = getAdminApiErrorMessage(cause)
      setError(message)
      notice.add({ title: '封面保存失败，请刷新确认后重试', description: message, color: 'error' })
    } finally {
      setBusy(false)
      onBusy(false)
    }
  }

  const openPicker = () => {
    setSelectedId(album.coverPhotoId || '')
    setQuery('')
    setPage(1)
    setError('')
    setPickerOpen(true)
  }

  const upload = (file: File) => {
    if (!isCoverUploadable(file.name)) {
      setError('请选择 PNG、JPG/JPEG 或 WebP 图片')
      return
    }
    const form = new FormData()
    // 封面单独上传的字段名是 `file`；相册照片上传用的是 `files`，两者不同。
    form.append('file', file)
    void save('POST', form)
  }

  return (
    <Card>
      <Card.Header>
        <Card.Title>
          <span style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            相册封面
            <Chip
              color={album.coverSource === 'auto' ? 'default' : 'accent'}
              variant="soft"
              size="sm"
            >
              {sourceLabel[album.coverSource] ?? '自动封面'}
            </Chip>
          </span>
        </Card.Title>
      </Card.Header>
      <Card.Content>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 24, alignItems: 'flex-start' }}>
          <div
            style={{
              width: 240,
              maxWidth: '100%',
              aspectRatio: '4 / 3',
              borderRadius: 10,
              overflow: 'hidden',
              border: '1px solid var(--border)',
              background: 'var(--surface-secondary)',
              display: 'grid',
              placeItems: 'center',
            }}
          >
            {album.coverUrl ? (
              <img
                src={album.coverUrl}
                alt="相册封面"
                style={{ width: '100%', height: '100%', objectFit: 'cover' }}
              />
            ) : (
              <span style={{ color: 'var(--muted)', fontSize: 14 }}>暂无封面</span>
            )}
          </div>

          <div style={{ flex: 1, minWidth: 240, display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
              <Button
                variant="primary"
                isDisabled={locked || !photos.length}
                onPress={openPicker}
              >
                从相册选择
              </Button>

              {/* 与现网一致：disabled && !busy —— 上传进行中时不禁用按钮，避免中途失去焦点。 */}
              <Button
                variant="secondary"
                isDisabled={disabled && !busy}
                onPress={() => fileInput.current?.click()}
              >
                从电脑上传
              </Button>
              <input
                ref={fileInput}
                type="file"
                accept=".png,.jpg,.jpeg,.webp"
                style={{ display: 'none' }}
                onChange={(event) => {
                  const file = event.target.files?.[0]
                  event.target.value = ''
                  if (file) upload(file)
                }}
              />

              <Button
                variant="ghost"
                isDisabled={locked || album.coverSource === 'auto'}
                onPress={async () => {
                  const ok = await notice.confirm('恢复自动封面？只移除手动封面设置，不会删除相册里的照片。', true)
                  if (ok) await save('DELETE')
                }}
              >
                恢复自动封面
              </Button>
            </div>

            {busy ? (
              <p className="admin-help" role="status">正在上传并保存封面，请稍候…</p>
            ) : null}

            {error ? <p className="admin-field-error">{error}</p> : null}

            <p className="admin-help">
              用于相册首页卡片和详情页背景。选择后立即保存，不改变相册内图片的顺序。
            </p>
            <p className="admin-help">
              支持 PNG、JPG/JPEG、WebP。单独上传的封面不计入照片数量，也不包含在下载包中。
            </p>
          </div>
        </div>
      </Card.Content>

      <Modal.Root
        isOpen={pickerOpen}
        onOpenChange={(next: boolean) => { if (!next && !busy) setPickerOpen(false) }}
      >
        <Modal.Backdrop isDismissable={!busy}>
          <Modal.Container size="lg">
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>从相册选择封面</Modal.Heading>
              </Modal.Header>
              <Modal.Body>
                <TextField
                  value={query}
                  isDisabled={busy}
                  onChange={(value: string) => { setQuery(value); setPage(1) }}
                >
                  <Input placeholder="搜索图片文件名" aria-label="搜索封面图片" />
                </TextField>

                {error ? (
                  <div style={{ marginTop: 12 }}>
                    <Alert status="danger">
                      <Alert.Content>
                        <Alert.Description>{error}</Alert.Description>
                      </Alert.Content>
                    </Alert>
                  </div>
                ) : null}

                <div style={{ marginTop: 16 }}>
                  {paged.length ? (
                    <div className="cover-picker">
                      {paged.map((photo) => (
                        <button
                          key={photo.id}
                          type="button"
                          className="cover-choice"
                          aria-pressed={selectedId === photo.id}
                          aria-label={`选择封面：${photo.originalName}`}
                          disabled={busy}
                          onClick={() => setSelectedId(photo.id)}
                        >
                          <img
                            src={`/api/photos/${encodeURIComponent(photo.id)}/thumbnail?v=grid2`}
                            alt=""
                            loading="lazy"
                            draggable={false}
                          />
                          <figcaption title={photo.originalName}>{photo.originalName}</figcaption>
                          {selectedId === photo.id ? (
                            <span className="cover-check"><Icon name="check" size={14} /></span>
                          ) : null}
                        </button>
                      ))}
                    </div>
                  ) : (
                    <p className="admin-help" style={{ padding: '24px 0', textAlign: 'center' }}>
                      没有匹配的图片
                    </p>
                  )}
                </div>
              </Modal.Body>
              <Modal.Footer>
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16, width: '100%', flexWrap: 'wrap' }}>
                  <span className="admin-help">
                    {selectedPhoto ? `已选择：${selectedPhoto.originalName}` : '请选择一张图片作为封面'}
                  </span>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
                    <Pager
                      page={page}
                      pageSize={PICKER_PAGE_SIZE}
                      total={filtered.length}
                      onChange={setPage}
                      disabled={busy}
                    />
                    <Button variant="ghost" isDisabled={busy} onPress={() => setPickerOpen(false)}>
                      取消
                    </Button>
                    <Button
                      variant="primary"
                      isDisabled={!selectedPhoto || busy}
                      onPress={() => void save('PUT', { photoId: selectedPhoto?.id })}
                    >
                      设为封面
                    </Button>
                  </div>
                </div>
              </Modal.Footer>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal.Root>
    </Card>
  )
}
