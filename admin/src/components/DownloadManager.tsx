import { useCallback, useEffect, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import {
  Alert, Button, Card, Checkbox, Chip, Input, Label, ListBoxItem, Modal, Radio, RadioGroup,
  Select, Switch, TextField,
} from '@heroui/react'
import { getAdminApiErrorMessage, adminApi } from '../lib/api'
import { adminBytes, downloadStatusText, downloadStatusTone, formatDateTime } from '../lib/format'
import { notice } from '../lib/notice'
import { useBeforeUnload } from '../lib/navigation'
import { Icon } from '../lib/icons'
import type { AdminAlbumDownloads, DownloadFormat } from '../lib/types'
import DataTable from './DataTable'
import type { Column } from './DataTable'
import PageHeader from './PageHeader'
import Pager from './Pager'

const FORMATS: DownloadFormat[] = ['png', 'jpg', 'jpeg', 'webp']
const LIST_PAGE_SIZE = 20
const JOB_PAGE_SIZE = 8

interface Draft {
  enabled: boolean
  formats: DownloadFormat[]
  imageMB: number
}

const draftOf = (config?: { enabled: boolean, formats: DownloadFormat[], maxImageBytes: number }): Draft => ({
  enabled: config?.enabled ?? true,
  // 必须浅拷贝：与 data.settings 共享同一个数组会让 dirty 判定永远相等。
  formats: [...(config?.formats ?? ['webp'])],
  imageMB: (config?.maxImageBytes ?? 5_000_000) / 1_000_000,
})

export interface DownloadManagerProps {
  /** 内嵌模式（相册页的「公开下载」页签）使用这个 props 指定相册。 */
  albumId?: string
  embedded?: boolean
  onDirty?: (dirty: boolean) => void
  onBusy?: (busy: boolean) => void
}

/**
 * 下载管理。独立页与相册内嵌两种上下文共用同一个组件，
 * 差异集中在：相册来源（query vs props）、是否渲染列表/页头、以及离开拦截的归属。
 */
export default function DownloadManager({
  albumId,
  embedded = false,
  onDirty,
  onBusy,
}: DownloadManagerProps) {
  const [searchParams, setSearchParams] = useSearchParams()

  const [data, setData] = useState<AdminAlbumDownloads | null>(null)
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [actionId, setActionId] = useState('')
  const [error, setError] = useState('')
  const [search, setSearch] = useState('')
  const [checkedAlbums, setCheckedAlbums] = useState<string[]>([])
  const [listPage, setListPage] = useState(1)
  const [jobPage, setJobPage] = useState(1)
  const [history, setHistory] = useState(false)
  const [draft, setDraft] = useState<Draft>(() => draftOf())
  const [saved, setSaved] = useState('')

  // 批量设置弹窗
  const [bulkOpen, setBulkOpen] = useState(false)
  const [bulkSaving, setBulkSaving] = useState(false)
  const [bulkScope, setBulkScope] = useState<'selected' | 'all'>('selected')
  const [bulkSelected, setBulkSelected] = useState<string[]>([])
  const [bulkDraft, setBulkDraft] = useState<Draft>(() => draftOf())
  const [bulkBaseline, setBulkBaseline] = useState('')

  // 内嵌时相册来自 props，独立页时 URL 是唯一真相。
  const selected = embedded ? (albumId || '') : (searchParams.get('album') || '')

  const dataRef = useRef<AdminAlbumDownloads | null>(null)
  const selectedRef = useRef(selected)
  const savedRef = useRef('')
  const loadRequest = useRef<Promise<void> | null>(null)

  dataRef.current = data
  selectedRef.current = selected

  const signature = JSON.stringify(draft)
  const dirty = Boolean(saved) && signature !== saved
  savedRef.current = saved

  const current = data?.settings.find((item) => item.albumId === selected)
  const bulkSignature = JSON.stringify([bulkScope, bulkSelected, bulkDraft])
  const bulkDirty = bulkOpen && bulkBaseline !== bulkSignature
  const bulkCount = bulkScope === 'all' ? (data?.settings.length ?? 0) : bulkSelected.length

  const apply = useCallback((config?: { enabled: boolean, formats: DownloadFormat[], maxImageBytes: number }) => {
    const next = draftOf(config)
    setDraft(next)
    // React 里 setState 后立刻读 state 会拿到旧值，因此显式用刚构造的对象算基线。
    setSaved(JSON.stringify(next))
  }, [])

  // silent=true 用于后台轮询：不切换 loading，否则「刷新」按钮会随 3 秒轮询一直闪。
  const load = useCallback(async (reset = false, silent = false): Promise<void> => {
    if (loadRequest.current) {
      await loadRequest.current
      if (reset) await load(true, silent)
      return
    }
    if (!silent) setLoading(true)
    const pending = (async () => {
      try {
        const value = await adminApi.adminFetch<AdminAlbumDownloads>('/api/album-downloads')
        setData(value)
        setError('')
        // 与最新设置求交集，防止已删除的相册残留在批量选择里。
        setCheckedAlbums((previous) =>
          previous.filter((id) => value.settings.some((item) => item.albumId === id)))
        // 轮询绝不能覆盖用户正在编辑的草稿：只在未初始化或显式 reset 时同步表单。
        if (reset || !savedRef.current) {
          const config = value.settings.find((item) => item.albumId === selectedRef.current)
          apply(config)
        }
      } catch (cause) {
        setError(getAdminApiErrorMessage(cause))
      } finally {
        if (!silent) setLoading(false)
        loadRequest.current = null
      }
    })()
    loadRequest.current = pending
    await pending
  }, [apply])

  // 递归 setTimeout：可见 3s / 隐藏 15s，且永不自动停止（只在卸载时结束）。
  useEffect(() => {
    let mounted = true
    let timer: ReturnType<typeof setTimeout> | undefined
    const poll = async () => {
      await load(false, true)
      if (!mounted) return
      timer = setTimeout(() => void poll(), document.hidden ? 15000 : 3000)
    }
    void poll()
    return () => { mounted = false; clearTimeout(timer) }
  }, [load])

  // 切换相册时重置本地编辑状态。
  useEffect(() => {
    setSaved('')
    savedRef.current = ''
    setHistory(false)
    setBulkOpen(false)
    setJobPage(1)
  }, [selected])

  useEffect(() => { onDirty?.(dirty) }, [dirty, onDirty])
  useEffect(() => { onBusy?.(saving || Boolean(actionId)) }, [saving, actionId, onBusy])

  // 两种上下文都注册 beforeunload（与现网一致）。
  useBeforeUnload(dirty || bulkDirty)

  const pick = (value: string) => {
    setSearchParams(value ? { album: value } : {}, { replace: false })
  }

  const save = async () => {
    if (!current || saving) return
    if (!draft.formats.length) {
      notice.add({ title: '请选择至少一种格式', color: 'warning' })
      return
    }
    setSaving(true)
    try {
      // 从开启改为关闭属于破坏性操作，需要额外确认（现网这里也是 danger 确认框）。
      if (!draft.enabled && current.enabled) {
        const ok = await notice.confirm(
          '关闭公开下载并删除该相册已生成的本地 ZIP？原始图片不受影响。',
          true,
        )
        if (!ok) return
      }
      await adminApi.adminFetch(`/api/albums/${selected}/download-settings`, {
        method: 'PUT',
        body: {
          enabled: draft.enabled,
          formats: draft.formats,
          maxImageBytes: Math.round((draft.imageMB || 0) * 1_000_000),
          maxZipBytes: 0,
        },
      })
      await load(true)
      notice.add({
        title: draft.enabled ? '已保存，系统将在后台生成压缩包' : '已关闭公开下载，本地压缩包将自动清理',
        color: 'success',
      })
    } catch (cause) {
      notice.add({ title: '保存失败', description: getAdminApiErrorMessage(cause), color: 'error' })
    } finally {
      setSaving(false)
    }
  }

  const rebuild = async () => {
    if (!current?.enabled || saving) return
    if (dirty) {
      notice.add({ title: '请先保存或放弃下载设置', color: 'warning' })
      return
    }
    setSaving(true)
    try {
      await adminApi.adminFetch(`/api/albums/${selected}/downloads/rebuild`, { method: 'POST' })
      await load()
      notice.add({ title: '已提交重新生成任务，可以离开页面', color: 'success' })
    } catch (cause) {
      notice.add({ title: '提交失败', description: getAdminApiErrorMessage(cause), color: 'error' })
    } finally {
      setSaving(false)
    }
  }

  const jobAction = async (id: string, action: 'cancel' | 'delete') => {
    // 单飞：同一时刻只允许一个任务操作在途。
    if (actionId) return
    setActionId(id)
    try {
      await adminApi.adminFetch(
        `/api/album-downloads/${id}${action === 'cancel' ? '/cancel' : ''}`,
        { method: action === 'cancel' ? 'POST' : 'DELETE' },
      )
      await load()
      notice.add({
        title: action === 'cancel' ? '已请求取消任务' : '已撤下下载，正在删除本地 ZIP',
        color: 'success',
      })
    } catch (cause) {
      notice.add({ title: '操作失败', description: getAdminApiErrorMessage(cause), color: 'error' })
    } finally {
      setActionId('')
    }
  }

  const openBulk = () => {
    if (!data?.settings.length || saving) return
    if (dirty) {
      notice.add({ title: '请先保存或重置当前相册的修改', color: 'warning' })
      return
    }
    // 预选优先级：列表勾选 > 当前相册 > 空。
    const preselect = checkedAlbums.length
      ? [...checkedAlbums]
      : selected ? [selected] : []
    const next = draftOf(current)
    setBulkScope('selected')
    setBulkSelected(preselect)
    setBulkDraft(next)
    setBulkBaseline(JSON.stringify(['selected', preselect, next]))
    setBulkOpen(true)
  }

  const closeBulk = async () => {
    if (bulkSaving) return
    if (bulkDirty) {
      const ok = await notice.confirm('批量设置尚未应用，确定放弃修改吗？')
      if (!ok) return
    }
    setBulkOpen(false)
  }

  const saveBulk = async () => {
    if (bulkSaving) return
    if (!bulkCount) {
      notice.add({ title: '请至少选择一个相册', color: 'warning' })
      return
    }
    if (!bulkDraft.formats.length) {
      notice.add({ title: '请选择至少一种图片格式', color: 'warning' })
      return
    }
    const known = data?.settings ?? []
    if (bulkScope === 'selected' && bulkSelected.some((id) => !known.some((item) => item.albumId === id))) {
      notice.add({ title: '选中的相册已不存在，请重新选择', color: 'warning' })
      return
    }

    const scopeText = bulkScope === 'all'
      ? `全部现有相册（当前 ${known.length} 个）`
      : `选中的 ${bulkSelected.length} 个相册`
    const sizeText = bulkDraft.imageMB ? `单张最多 ${bulkDraft.imageMB} MB` : '不限大小'
    const ok = await notice.confirm(
      `将覆盖${scopeText}的下载开关、图片格式和大小上限：${bulkDraft.enabled ? '开启下载' : '关闭下载'}，`
      + `${bulkDraft.formats.join(' / ').toUpperCase()}，${sizeText}。旧 ZIP 将清理`
      + `${bulkDraft.enabled ? '并在后台重新生成' : ''}，原始图片不受影响。确定应用？`,
      true,
    )
    if (!ok) return

    setBulkSaving(true)
    try {
      const settings = {
        enabled: bulkDraft.enabled,
        formats: [...bulkDraft.formats],
        maxImageBytes: Math.round((bulkDraft.imageMB || 0) * 1_000_000),
        maxZipBytes: 0,
      }
      // 后端是 internally-tagged enum + deny_unknown_fields：
      // scope='all' 时绝不能带上 albumIds 键，否则直接 400。
      const target = bulkScope === 'all'
        ? { scope: 'all' }
        : { scope: 'selected', albumIds: [...bulkSelected] }
      const result = await adminApi.adminFetch<{ updated: number }>(
        '/api/album-downloads/settings/bulk',
        { method: 'PUT', body: { target, settings } },
      )
      setBulkOpen(false)
      await load(true)
      notice.add({
        title: `已统一设置 ${result.updated} 个相册`,
        description: bulkDraft.enabled
          ? '压缩包将在后台自动更新，可以离开页面。'
          : '已关闭公开下载，本地 ZIP 将自动清理；原始图片不受影响。',
        color: 'success',
      })
    } catch (cause) {
      notice.add({ title: '批量设置失败', description: getAdminApiErrorMessage(cause), color: 'error' })
    } finally {
      setBulkSaving(false)
    }
  }

  /** 当前版本状态推导：需处理 > 进行中 > 可下载 > 未就绪。 */
  const albumStatus = (id: string): string => {
    const config = data?.settings.find((item) => item.albumId === id)
    if (!config) return '—'
    if (!config.enabled) return '未开启'
    const currentJobs = (data?.jobs ?? []).filter(
      (job) => job.albumId === config.albumId && job.revision === config.revision,
    )
    if (currentJobs.some((job) => ['failed', 'interrupted'].includes(job.status))) return '需处理'
    if (currentJobs.some((job) => ['running', 'queued', 'deleting'].includes(job.status))) return '进行中'
    if (currentJobs.length && currentJobs.every((job) => job.status === 'ready')) return '可下载'
    return '未就绪'
  }

  const statusTone = (status: string): 'success' | 'warning' | 'danger' | 'accent' | 'default' => {
    if (status === '需处理') return 'danger'
    if (status === '进行中') return 'accent'
    if (status === '可下载') return 'success'
    if (status === '未就绪') return 'warning'
    return 'default'
  }

  // 当前版本的 job；历史记录开关打开时展示全部版本。
  const currentRevision = current?.revision
  const allJobs = (data?.jobs ?? []).filter((job) => job.albumId === selected)
  const jobs = allJobs.filter((job) => history || job.revision === currentRevision)

  const filteredSettings = (data?.settings ?? []).filter((item) =>
    item.albumName.toLocaleLowerCase().includes(search.toLocaleLowerCase()))
  const pagedSettings = filteredSettings.slice((listPage - 1) * LIST_PAGE_SIZE, listPage * LIST_PAGE_SIZE)
  const pagedJobs = jobs.slice((jobPage - 1) * JOB_PAGE_SIZE, jobPage * JOB_PAGE_SIZE)

  const toggleFormat = (format: DownloadFormat, checked: boolean, target: 'single' | 'bulk') => {
    const setter = target === 'single' ? setDraft : setBulkDraft
    setter((previous) => ({
      ...previous,
      formats: checked
        ? [...new Set([...previous.formats, format])]
        : previous.formats.filter((item) => item !== format),
    }))
  }

  const settingsForm = (value: Draft, setter: (updater: (previous: Draft) => Draft) => void, target: 'single' | 'bulk', disabled: boolean) => (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <Switch
        isSelected={value.enabled}
        isDisabled={disabled}
        onChange={(enabled: boolean) => setter((previous) => ({ ...previous, enabled }))}
      >
        公开下载{value.enabled ? '已开启' : '已关闭'}
      </Switch>

      <div>
        <div style={{ fontSize: 13, color: 'var(--muted)', marginBottom: 8 }}>
          图片格式（每种格式生成一个独立 ZIP。JPG 与 JPEG 编码相同，扩展名不同。）
        </div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 16 }}>
          {FORMATS.map((format) => (
            <Checkbox
              key={format}
              isSelected={value.formats.includes(format)}
              isDisabled={disabled}
              onChange={(checked: boolean) => toggleFormat(format, checked, target)}
            >
              {format.toUpperCase()}
            </Checkbox>
          ))}
        </div>
      </div>

      <TextField
        value={String(value.imageMB)}
        isDisabled={disabled}
        onChange={(next: string) => {
          const parsed = Number.parseFloat(next)
          setter((previous) => ({ ...previous, imageMB: Number.isFinite(parsed) ? parsed : 0 }))
        }}
      >
        <Label>单张图片上限（MB）</Label>
        <Input type="number" min={0} max={500} step={0.5} />
      </TextField>
      <p className="admin-help">
        0 表示不限。必要时降低画质或分辨率；PNG 保持无损编码，通过缩小尺寸达标。
      </p>
    </div>
  )

  const listColumns: Column<typeof filteredSettings[number]>[] = [
    {
      key: 'album',
      title: '相册',
      render: (item) => (
        <button
          type="button"
          onClick={() => pick(item.albumId)}
          style={{
            background: 'none',
            border: 0,
            padding: 0,
            cursor: 'pointer',
            color: 'var(--link)',
            fontSize: 14,
            textAlign: 'left',
          }}
        >
          {item.albumName}
        </button>
      ),
    },
    {
      key: 'enabled',
      title: '公开下载',
      width: 130,
      render: (item) => (
        <Chip color={item.enabled ? 'success' : 'default'} variant="soft" size="sm">
          {item.enabled ? '已开启' : '未开启'}
        </Chip>
      ),
    },
    {
      key: 'formats',
      title: '格式',
      width: 220,
      render: (item) => (
        <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
          {item.formats.map((format) => (
            <Chip key={format} variant="tertiary" size="sm">{format.toUpperCase()}</Chip>
          ))}
        </div>
      ),
    },
    {
      key: 'status',
      title: '当前任务',
      width: 130,
      render: (item) => {
        const status = albumStatus(item.albumId)
        return <Chip color={statusTone(status)} variant="soft" size="sm">{status}</Chip>
      },
    },
    {
      key: 'actions',
      title: '操作',
      width: 110,
      render: (item) => (
        <button
          type="button"
          onClick={() => pick(item.albumId)}
          style={{ background: 'none', border: 0, padding: 0, cursor: 'pointer', color: 'var(--link)', fontSize: 14 }}
        >
          管理下载
        </button>
      ),
    },
  ]

  const jobColumns: Column<typeof jobs[number]>[] = [
    {
      key: 'format',
      title: '格式',
      width: 90,
      render: (job) => (
        <div>
          <div>{job.format.toUpperCase()}</div>
          <div className="admin-help">v{job.revision}</div>
        </div>
      ),
    },
    {
      key: 'status',
      title: '状态 / 进度',
      width: 200,
      render: (job) => (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <Chip color={downloadStatusTone[job.status] ?? 'default'} variant="soft" size="sm">
            {downloadStatusText[job.status] || job.status}
          </Chip>
          {job.status === 'running' ? (
            <span className="admin-help">
              {job.completed} / {job.total}
            </span>
          ) : null}
          {job.error ? <span className="admin-field-error">{job.error}</span> : null}
        </div>
      ),
    },
    {
      key: 'size',
      title: '文件大小',
      width: 110,
      render: (job) => (job.byteSize ? adminBytes(job.byteSize) : '—'),
    },
    {
      key: 'createdAt',
      title: '生成时间',
      width: 175,
      render: (job) => formatDateTime(job.createdAt),
    },
    {
      key: 'actions',
      title: '操作',
      width: 170,
      render: (job) => (
        <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
          {job.status === 'ready' && job.revision === currentRevision && current?.enabled ? (
            // 原生链接导航触发下载，不走 fetch。
            <a
              href={`/api/albums/${job.albumId}/downloads/${job.format}?version=${job.id}`}
              style={{ color: 'var(--link)', fontSize: 14 }}
            >
              下载
            </a>
          ) : null}
          {['queued', 'running'].includes(job.status) ? (
            <button
              type="button"
              disabled={Boolean(actionId)}
              onClick={() => void jobAction(job.id, 'cancel')}
              style={{ background: 'none', border: 0, padding: 0, cursor: 'pointer', color: 'var(--link)', fontSize: 14 }}
            >
              取消
            </button>
          ) : null}
          {!['deleted', 'deleting'].includes(job.status) ? (
            <button
              type="button"
              disabled={Boolean(actionId)}
              onClick={async () => {
                const ok = await notice.confirm(
                  '删除本地压缩包？原始图片不会删除，可随时重新生成。',
                  true,
                )
                if (ok) await jobAction(job.id, 'delete')
              }}
              style={{ background: 'none', border: 0, padding: 0, cursor: 'pointer', color: 'var(--danger)', fontSize: 14 }}
            >
              删除
            </button>
          ) : null}
        </div>
      ),
    },
  ]

  return (
    <div className="admin-stack">
      {!embedded ? (
        <PageHeader
          title="下载管理"
          description="为每个相册单独设置公开下载格式与体积上限，压缩包在服务器本地生成。"
          actions={
            <>
              {selected ? (
                <Button variant="secondary" onPress={() => pick('')}>返回下载列表</Button>
              ) : null}
              <Button variant="secondary" isDisabled={loading} onPress={() => void load()}>
                <Icon name="refresh" size={16} />
                刷新
              </Button>
            </>
          }
        />
      ) : null}

      {error ? (
        <Alert status="danger">
          <Alert.Indicator />
          <Alert.Content>
            <Alert.Description>
              <span className="preserve-newlines">{error}</span>
            </Alert.Description>
          </Alert.Content>
        </Alert>
      ) : null}

      {/* 列表视图：仅独立页、且未选中相册时显示 */}
      {!embedded && !selected ? (
        <Card>
          <Card.Content>
            <div className="admin-toolbar" style={{ marginBottom: 16 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                <TextField value={search} onChange={setSearch} className="max-w-[70vw]">
                  <Input placeholder="搜索相册" aria-label="搜索相册" />
                </TextField>
                <span className="admin-help">{filteredSettings.length} 个相册</span>
              </div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                <div style={{ textAlign: 'right' }}>
                  <div style={{ fontSize: 13, color: 'var(--muted)' }}>本地 ZIP 占用</div>
                  <div style={{ fontSize: 20, fontWeight: 600 }}>{adminBytes(data?.localBytes ?? 0)}</div>
                </div>
                <Button
                  variant="primary"
                  isDisabled={!data?.settings.length}
                  onPress={openBulk}
                >
                  {checkedAlbums.length ? `批量设置（${checkedAlbums.length}）` : '批量设置'}
                </Button>
                {checkedAlbums.length ? (
                  <Button variant="ghost" onPress={() => setCheckedAlbums([])}>取消选择</Button>
                ) : null}
              </div>
            </div>

            <DataTable
              columns={listColumns}
              rows={pagedSettings}
              rowKey={(item) => item.albumId}
              loading={loading}
              emptyText="请先创建相册，再配置公开下载。"
              minWidth={750}
              selection={{ selectedKeys: checkedAlbums, onChange: setCheckedAlbums }}
            />
            <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 16 }}>
              <Pager
                page={listPage}
                pageSize={LIST_PAGE_SIZE}
                total={filteredSettings.length}
                onChange={setListPage}
              />
            </div>
          </Card.Content>
        </Card>
      ) : null}

      {/* 详情视图 */}
      {(!embedded && selected) || embedded ? (
        <>
          {!embedded ? (
            <Card>
              <Card.Content>
                <div className="admin-toolbar">
                  <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                    <Select
                      aria-label="切换相册"
                      className="w-[260px] max-w-[65vw]"
                      selectedKey={selected}
                      isDisabled={saving}
                      onSelectionChange={(key) => pick(String(key))}
                    >
                      {(data?.settings ?? []).map((item) => (
                        <ListBoxItem key={item.albumId} id={item.albumId}>
                          {item.albumName}
                        </ListBoxItem>
                      ))}
                    </Select>
                    <Button variant="secondary" isDisabled={!data?.settings.length} onPress={openBulk}>
                      批量设置
                    </Button>
                    <Link
                      to={`/albums?album=${encodeURIComponent(selected)}`}
                      style={{ color: 'var(--link)', fontSize: 14 }}
                    >
                      管理此相册图片 →
                    </Link>
                  </div>
                </div>
              </Card.Content>
            </Card>
          ) : null}

          {selected && !current && !loading && !error ? (
            <Alert status="warning">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Description>此相册不存在，请返回列表重新选择。</Alert.Description>
              </Alert.Content>
            </Alert>
          ) : null}

          {!data?.settings.length && !loading ? (
            <Alert status="accent">
              <Alert.Content>
                <Alert.Description>请先创建相册，再配置公开下载。</Alert.Description>
              </Alert.Content>
            </Alert>
          ) : null}

          {current ? (
            <Card>
              <Card.Header>
                <Card.Title>公开下载设置 · {current.albumName}</Card.Title>
              </Card.Header>
              <Card.Content>
                <form
                  onSubmit={(event) => { event.preventDefault(); void save() }}
                  style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
                >
                  {settingsForm(draft, setDraft, 'single', saving)}
                  <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', alignItems: 'center' }}>
                    <Button type="submit" variant="primary" isDisabled={!dirty || saving}>
                      保存下载设置
                    </Button>
                    <Button
                      variant="ghost"
                      isDisabled={!dirty || saving}
                      onPress={() => apply(current)}
                    >
                      放弃修改
                    </Button>
                    {dirty ? <span className="admin-unsaved">有未保存的修改</span> : null}
                  </div>
                </form>
              </Card.Content>
            </Card>
          ) : null}

          {current ? (
            <Card>
              <Card.Header>
                <Card.Title>
                  <span style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                    本地压缩包
                    <Checkbox
                      isSelected={history}
                      onChange={setHistory}
                    >
                      显示历史记录
                    </Checkbox>
                  </span>
                </Card.Title>
              </Card.Header>
              <Card.Content>
                <div className="admin-toolbar" style={{ marginBottom: 16 }}>
                  <span className="admin-help">
                    {history ? '全部版本（含历史记录）' : '当前版本'}
                  </span>
                  <Button
                    variant="secondary"
                    isDisabled={!current.enabled || saving}
                    onPress={() => void rebuild()}
                  >
                    重新生成
                  </Button>
                </div>

                <DataTable
                  columns={jobColumns}
                  rows={pagedJobs}
                  rowKey={(job) => job.id}
                  loading={loading}
                  emptyText="当前版本还没有压缩包"
                  minWidth={700}
                />
                <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 16 }}>
                  <Pager
                    page={jobPage}
                    pageSize={JOB_PAGE_SIZE}
                    total={jobs.length}
                    onChange={setJobPage}
                  />
                </div>

                <p className="admin-help" style={{ marginTop: 16 }}>
                  生成任务在服务器后台运行，离开页面不受影响。增删图片、改名后自动更新；ZIP
                  仅存本机，不占用 S3 / WebDAV。
                </p>
                <p className="admin-help" style={{ marginTop: 8 }}>
                  存放位置：{data?.directory}。删除后当前版本不会自动重建，点击“重新生成”即可恢复。
                </p>
              </Card.Content>
            </Card>
          ) : null}
        </>
      ) : null}

      {/* 批量设置 */}
      <Modal.Root
        isOpen={bulkOpen}
        onOpenChange={(next: boolean) => { if (!next) void closeBulk() }}
      >
        <Modal.Backdrop isDismissable={!bulkSaving}>
          <Modal.Container size="lg">
            <Modal.Dialog>
              <Modal.Header>
                <Modal.Heading>批量设置相册下载</Modal.Heading>
              </Modal.Header>
              <Modal.Body>
                <Alert status="warning">
                  <Alert.Indicator />
                  <Alert.Content>
                    <Alert.Title>统一覆盖各相册的单独设置</Alert.Title>
                    <Alert.Description>
                      下载开关、图片格式和大小上限都会替换为下方的设置，不会合并。只影响本次指定的现有相册，之后新建的相册不受影响。
                    </Alert.Description>
                  </Alert.Content>
                </Alert>

                <div style={{ marginTop: 16 }}>
                  <RadioGroup
                    value={bulkScope}
                    onChange={(value) => setBulkScope(value as 'selected' | 'all')}
                    isDisabled={bulkSaving}
                  >
                    <Radio value="selected">勾选多个相册</Radio>
                    <Radio value="all">覆盖全部现有相册（当前 {data?.settings.length ?? 0} 个）</Radio>
                  </RadioGroup>
                </div>

                {bulkScope === 'selected' ? (
                  <div style={{ marginTop: 16, display: 'flex', flexDirection: 'column', gap: 8 }}>
                    <span className="admin-help">选择要应用的相册</span>
                    <div
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 8,
                        maxHeight: 200,
                        overflowY: 'auto',
                        border: '1px solid var(--border)',
                        borderRadius: 8,
                        padding: 12,
                      }}
                    >
                      {(data?.settings ?? []).map((item) => (
                        <Checkbox
                          key={item.albumId}
                          isSelected={bulkSelected.includes(item.albumId)}
                          isDisabled={bulkSaving}
                          onChange={(checked: boolean) => setBulkSelected((previous) => (
                            checked
                              ? [...new Set([...previous, item.albumId])]
                              : previous.filter((id) => id !== item.albumId)
                          ))}
                        >
                          {item.albumName}
                        </Checkbox>
                      ))}
                    </div>
                  </div>
                ) : null}

                <div style={{ marginTop: 20 }}>
                  {settingsForm(bulkDraft, setBulkDraft, 'bulk', bulkSaving)}
                </div>
              </Modal.Body>
              <Modal.Footer>
                <Button variant="ghost" isDisabled={bulkSaving} onPress={() => void closeBulk()}>
                  取消
                </Button>
                <Button
                  variant="primary"
                  isDisabled={!bulkCount || !bulkDraft.formats.length || bulkSaving}
                  onPress={() => void saveBulk()}
                >
                  覆盖并应用（{bulkCount}）
                </Button>
              </Modal.Footer>
            </Modal.Dialog>
          </Modal.Container>
        </Modal.Backdrop>
      </Modal.Root>
    </div>
  )
}
