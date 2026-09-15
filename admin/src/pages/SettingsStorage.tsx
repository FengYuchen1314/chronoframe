import { useCallback, useEffect, useRef, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import {
  Alert, Button, Card, Chip, Input, Label, ProgressBar, Radio, RadioGroup, Tabs, TextField,
} from '@heroui/react'
import { adminApi, getAdminApiErrorMessage } from '../lib/api'
import {
  formatBytes, formatTime, jobProgressPercent, migrationStatusText, migrationStatusTone,
  progressPercent, s3CleanupStatusText, s3CleanupStatusTone, thumbnailStatusText,
  thumbnailStatusTone,
} from '../lib/format'
import { notice } from '../lib/notice'
import { useLeaveConfirm } from '../lib/navigation'
import { Icon } from '../lib/icons'
import type {
  Album, S3CleanupJob, StorageBackend, StorageMigrationJob, StorageSettings,
  StorageSettingsInput, ThumbnailRebuildJob,
} from '../lib/types'
import PageHeader from '../components/PageHeader'
import { useDocumentTitle } from '../lib/hooks'

const BACKENDS: Array<{ value: StorageBackend, label: string }> = [
  { value: 'local', label: '本地存储' },
  { value: 'webdav', label: 'WebDAV' },
  { value: 's3', label: 'S3 对象存储' },
]

const TABS = [
  { id: 'connection', label: '存储连接' },
  { id: 'migration', label: '存储迁移' },
  { id: 'cache', label: '图片缓存' },
  { id: 'cleanup', label: 'S3 空间清理' },
]

interface StorageForm {
  backend: StorageBackend
  localPath: string
  webdavUrl: string
  webdavUsername: string
  webdavPrefix: string
  s3Endpoint: string
  s3Region: string
  s3Bucket: string
  s3AccessKey: string
  s3Prefix: string
}

const EMPTY_FORM: StorageForm = {
  backend: 'local',
  localPath: './data/storage',
  webdavUrl: '',
  webdavUsername: '',
  webdavPrefix: 'chronoframe',
  s3Endpoint: '',
  s3Region: 'us-east-1',
  s3Bucket: '',
  s3AccessKey: '',
  s3Prefix: 'chronoframe',
}

/** 表单签名不含密码，因此只填密码不会让「与已保存一致」的判定失真。 */
const signatureOf = (form: StorageForm) => JSON.stringify({
  backend: form.backend,
  localPath: form.localPath,
  webdavUrl: form.webdavUrl,
  webdavUsername: form.webdavUsername,
  webdavPrefix: form.webdavPrefix,
  s3Endpoint: form.s3Endpoint,
  s3Region: form.s3Region,
  s3Bucket: form.s3Bucket,
  s3AccessKey: form.s3AccessKey,
  s3Prefix: form.s3Prefix,
})

/**
 * 存储位置的身份签名：只有「这个后端指向哪个位置」决定是否需要迁移。
 * 用户名 / 密码 / 密钥变化不算换位置，不会触发迁移。
 */
const targetOf = (form: StorageForm) => {
  if (form.backend === 'local') {
    return JSON.stringify({ backend: 'local', localPath: form.localPath.trim() })
  }
  if (form.backend === 'webdav') {
    return JSON.stringify({
      backend: 'webdav',
      url: form.webdavUrl.trim(),
      prefix: form.webdavPrefix.trim(),
    })
  }
  return JSON.stringify({
    backend: 's3',
    endpoint: form.s3Endpoint.trim(),
    region: form.s3Region.trim(),
    bucket: form.s3Bucket.trim(),
    prefix: form.s3Prefix.trim(),
  })
}

/**
 * 「任务是否在跑」的纯函数判定。
 *
 * 轮询里不能用渲染期的布尔值来判断「忙 → 闲」的转换：setState 要等 React
 * 重渲染才生效，而 `await` 一返回就读取只会拿到旧值，条件永远不成立。
 * 因此统一从「刚取回的数据」推算。
 */
const isBusyState = (
  jobs: StorageMigrationJob[],
  cleanup: S3CleanupJob | null,
  thumbnail: ThumbnailRebuildJob | null,
) => ({
  storage: jobs.some((job) =>
    ['queued', 'running'].includes(job.status) || job.cleanupStatus === 'cleaning')
    || cleanup?.status === 'running',
  thumbnail: Boolean(thumbnail && ['queued', 'running'].includes(thumbnail.status)),
})

export default function SettingsStorage() {
  useDocumentTitle('存储设置')
  const [searchParams, setSearchParams] = useSearchParams()

  const rawTab = String(searchParams.get('tab') || '')
  const tab = ['migration', 'cache', 'cleanup'].includes(rawTab) ? rawTab : 'connection'

  const [form, setForm] = useState<StorageForm>(EMPTY_FORM)
  const [webdavPassword, setWebdavPassword] = useState('')
  const [s3SecretKey, setS3SecretKey] = useState('')
  const [webdavPasswordSet, setWebdavPasswordSet] = useState(false)
  const [s3SecretKeySet, setS3SecretKeySet] = useState(false)
  const [savedBackend, setSavedBackend] = useState<StorageBackend>('local')
  const [savedSignature, setSavedSignature] = useState('')
  const [savedTargetSignature, setSavedTargetSignature] = useState('')
  // 首屏设置是否已成功加载。脏判定、离开确认、保存按钮都以它为前置条件，
  // 避免在拿到基线之前就允许保存（那会把未加载出的表单当成新配置提交）。
  const [loaded, setLoaded] = useState(false)

  const [migrationJobs, setMigrationJobs] = useState<StorageMigrationJob[]>([])
  const [latestThumbnailJob, setLatestThumbnailJob] = useState<ThumbnailRebuildJob | null>(null)
  const [latestS3Cleanup, setLatestS3Cleanup] = useState<S3CleanupJob | null>(null)
  const [storedPhotoCount, setStoredPhotoCount] = useState(0)
  const [lastTest, setLastTest] = useState<{ backend: StorageBackend, at: Date } | null>(null)

  const [isLoading, setIsLoading] = useState(false)
  const [isTesting, setIsTesting] = useState(false)
  const [isSaving, setIsSaving] = useState(false)
  const [isStorageTaskAction, setIsStorageTaskAction] = useState(false)
  const [isThumbnailTaskAction, setIsThumbnailTaskAction] = useState(false)
  const [isS3CleanupAction, setIsS3CleanupAction] = useState(false)

  const [loadError, setLoadError] = useState('')
  const [migrationLoadError, setMigrationLoadError] = useState('')
  const [thumbnailLoadError, setThumbnailLoadError] = useState('')
  const [s3CleanupLoadError, setS3CleanupLoadError] = useState('')

  const mounted = useRef(false)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const settingsInFlight = useRef(false)
  // 轮询世代号：StrictMode 下 effect 会执行两次，没有它就会出现两条轮询链，
  // 而 timer ref 只能记住最后一条，另一条永远清不掉。
  const pollGeneration = useRef(0)
  // 三个任务的最新快照。加载器在 await 返回时同步写入，不依赖渲染。
  const migrationsRef = useRef<StorageMigrationJob[]>([])
  const cleanupRef = useRef<S3CleanupJob | null>(null)
  const thumbnailRef = useRef<ThumbnailRebuildJob | null>(null)
  // 立即重排下一次轮询。任务动作结束后调用它，否则「忙 → 闲」要等到早已按
  // 空闲排好的 30 秒定时器触发才会被发现，脏标记的清除会显得很迟钝。
  const pollRef = useRef<(() => void) | null>(null)

  /* ------------------------------ 派生状态 ------------------------------ */

  const formSignature = signatureOf(form)
  // 密码框里有内容就算脏——无从判断它与已保存的值是否相同。
  // 必须等首屏加载成功才可能为脏：否则 savedSignature 还是空串，
  // 页面一打开就显示「有未保存的修改」，离开时还会无端弹确认框。
  const isDirty = loaded
    && (formSignature !== savedSignature || Boolean(webdavPassword) || Boolean(s3SecretKey))
  const storageTargetChanged = targetOf(form) !== savedTargetSignature
  const latestMigration = migrationJobs[0] || null

  const activeStorageTask = migrationJobs.find((job) =>
    ['queued', 'running'].includes(job.status) || job.cleanupStatus === 'cleaning') || null
  const s3CleanupActive = latestS3Cleanup?.status === 'running'
  const thumbnailTaskActive = Boolean(
    latestThumbnailJob && ['queued', 'running'].includes(latestThumbnailJob.status),
  )
  // 任一存储任务在跑就锁住整个「存储连接」表单。
  const storageBusy = Boolean(activeStorageTask || s3CleanupActive)
  const migrationRequired = storageTargetChanged && storedPhotoCount > 0

  useLeaveConfirm(isDirty, '存储设置尚未保存，确定放弃修改并离开吗？')

  /* -------------------------------- 动作 -------------------------------- */

  const clearSensitiveInputs = () => {
    setWebdavPassword('')
    setS3SecretKey('')
  }

  const applySettings = useCallback((settings: StorageSettings) => {
    const next: StorageForm = {
      backend: settings.backend,
      localPath: settings.localPath || './data/storage',
      webdavUrl: settings.webdavUrl,
      webdavUsername: settings.webdavUsername,
      webdavPrefix: settings.webdavPrefix || 'chronoframe',
      s3Endpoint: settings.s3Endpoint,
      s3Region: settings.s3Region || 'us-east-1',
      s3Bucket: settings.s3Bucket,
      s3AccessKey: settings.s3AccessKey,
      s3Prefix: settings.s3Prefix || 'chronoframe',
    }
    setForm(next)
    setWebdavPasswordSet(settings.webdavPasswordSet)
    setS3SecretKeySet(settings.s3SecretKeySet)
    setSavedBackend(settings.backend)
    setSavedSignature(signatureOf(next))
    setSavedTargetSignature(targetOf(next))
    setLoaded(true)
    clearSensitiveInputs()
    // 表单内容刚被服务端值覆盖，之前那次「连接测试通过」指向的已经不是屏幕上
    // 这套配置了，必须清掉。放在这里而不是 loadSettings 里：轮询在迁移结束后会
    // 静默重载设置，那条路径同样会覆盖表单。saveSettings 在本函数之后才写入
    // 新的 lastTest，顺序上不会被这里清掉。
    setLastTest(null)
  }, [])

  const buildPayload = (): StorageSettingsInput => ({
    backend: form.backend,
    localPath: form.localPath.trim(),
    webdavUrl: form.webdavUrl.trim(),
    webdavUsername: form.webdavUsername.trim(),
    // 留空时传 undefined：JSON 会丢掉这个键，后端收到 None 就保持原值。
    // 也就是说系统里没有「留空即清空密钥」这条路径。
    webdavPassword: webdavPassword || undefined,
    webdavPrefix: form.webdavPrefix.trim(),
    s3Endpoint: form.s3Endpoint.trim(),
    s3Region: form.s3Region.trim(),
    s3Bucket: form.s3Bucket.trim(),
    s3AccessKey: form.s3AccessKey.trim(),
    s3SecretKey: s3SecretKey || undefined,
    s3Prefix: form.s3Prefix.trim(),
  })

  // silent=true 时不切换 loading：后台轮询不应该让「刷新」按钮每 30 秒闪一次。
  const loadSettings = useCallback(async (silent = false) => {
    if (settingsInFlight.current) return
    settingsInFlight.current = true
    if (!silent) setIsLoading(true)
    setLoadError('')
    try {
      const [settings, albums] = await Promise.all([
        adminApi.adminFetch<StorageSettings>('/api/settings/storage'),
        adminApi.adminFetch<Album[]>('/api/albums'),
      ])
      applySettings(settings)
      setStoredPhotoCount(albums.reduce((total, album) => total + album.photoCount, 0))
    } catch (cause) {
      setLoadError(getAdminApiErrorMessage(cause))
    } finally {
      if (!silent) setIsLoading(false)
      settingsInFlight.current = false
    }
  }, [applySettings])

  // 三个加载器都返回数据并把最新值同步写入 ref，供轮询做「忙 → 闲」判定。
  const loadMigrations = useCallback(async (): Promise<StorageMigrationJob[]> => {
    try {
      const value = await adminApi.adminFetch<StorageMigrationJob[]>('/api/storage-migrations')
      migrationsRef.current = value
      setMigrationJobs(value)
      setMigrationLoadError('')
      return value
    } catch (cause) {
      setMigrationLoadError(getAdminApiErrorMessage(cause))
      return migrationsRef.current
    }
  }, [])

  const loadThumbnailJob = useCallback(async (): Promise<ThumbnailRebuildJob | null> => {
    try {
      const value = await adminApi.adminFetch<ThumbnailRebuildJob | null>(
        '/api/thumbnails/rebuilds/latest',
      )
      thumbnailRef.current = value
      setLatestThumbnailJob(value)
      setThumbnailLoadError('')
      return value
    } catch (cause) {
      setThumbnailLoadError(getAdminApiErrorMessage(cause))
      return thumbnailRef.current
    }
  }, [])

  const loadS3Cleanup = useCallback(async (): Promise<S3CleanupJob | null> => {
    try {
      const value = await adminApi.adminFetch<S3CleanupJob | null>('/api/s3-cleanups/latest')
      cleanupRef.current = value
      setLatestS3Cleanup(value)
      setS3CleanupLoadError('')
      return value
    } catch (cause) {
      setS3CleanupLoadError(getAdminApiErrorMessage(cause))
      return cleanupRef.current
    }
  }, [])

  const testConnection = async () => {
    if (isTesting || isSaving) return
    const payload = buildPayload()
    // 密码只发送这一次，输入框立即清空。
    clearSensitiveInputs()
    setIsTesting(true)
    setLastTest(null)
    try {
      await adminApi.adminFetch('/api/settings/storage/test', { method: 'POST', body: payload })
      setLastTest({ backend: payload.backend, at: new Date() })
      notice.add({ title: '存储连接测试通过', description: '本次测试不会保存配置。', color: 'success' })
    } catch (cause) {
      notice.add({
        title: '存储连接测试失败',
        description: getAdminApiErrorMessage(cause),
        color: 'error',
      })
    } finally {
      clearSensitiveInputs()
      setIsTesting(false)
    }
  }

  const saveSettings = async () => {
    if (isTesting || isSaving) return
    if (migrationRequired) {
      const ok = await notice.confirm(
        `确认将 ${storedPhotoCount} 张图片迁移到新的存储位置？\n\n`
        + '迁移会在后台复制并读回校验；完成后才切换存储。随后请在本页确认删除旧空间，或明确选择保留备份。',
      )
      if (!ok) return
    }
    const payload = buildPayload()
    clearSensitiveInputs()
    setIsSaving(true)
    try {
      if (migrationRequired) {
        await adminApi.adminFetch('/api/storage-migrations', { method: 'POST', body: payload })
        notice.add({
          title: '存储迁移已开始',
          description: '可以离开此页面；任务会持久化记录进度，服务重启后可手动继续。',
          color: 'success',
        })
        await loadMigrations()
        pollRef.current?.()
      } else {
        const saved = await adminApi.adminFetch<StorageSettings>('/api/settings/storage', {
          method: 'PUT',
          body: payload,
        })
        applySettings(saved)
        setLastTest({ backend: saved.backend, at: new Date() })
        notice.add({
          title: '存储设置已保存',
          description: '后端已验证连接并将该配置设为唯一活动存储。',
          color: 'success',
        })
      }
    } catch (cause) {
      notice.add({
        title: '存储设置保存失败',
        description: getAdminApiErrorMessage(cause),
        color: 'error',
      })
    } finally {
      clearSensitiveInputs()
      setIsSaving(false)
    }
  }

  const runStorageTaskAction = async (
    job: StorageMigrationJob,
    action: 'resume' | 'cancel' | 'cleanup' | 'retain',
  ) => {
    if (isStorageTaskAction) return
    if (action === 'cleanup') {
      // 标红是**有意**偏离现网（现网三处 confirm 全用默认样式）：删除旧存储副本
      // 不可撤销。与之相对，下面的 retain 是安全选项，必须保持默认样式。
      const ok = await notice.confirm(
        `确定删除迁移前 ${job.sourceBackend.toUpperCase()} 存储中的全部旧图片吗？\n\n`
        + '系统会逐张校验当前存储中的副本后再删除，但删除动作不能撤销。',
        true,
      )
      if (!ok) return
    }
    if (action === 'retain') {
      // 保留旧副本是**安全**选项，确认按钮不能标红：现网这里用的是默认样式，
      // 把它渲染成危险操作会让用户以为保留备份有风险，从而误选「删除」。
      const ok = await notice.confirm(
        '确定保留旧存储中的图片吗？\n\n系统会结束本次迁移流程，不会删除旧副本。',
      )
      if (!ok) return
    }
    setIsStorageTaskAction(true)
    try {
      await adminApi.adminFetch(`/api/storage-migrations/${job.id}/${action}`, { method: 'POST' })
      const titles: Record<typeof action, string> = {
        cleanup: '已开始清理旧存储',
        retain: '已保留旧存储',
        resume: '已继续迁移',
        cancel: '已请求安全中断',
      }
      notice.add({ title: titles[action], color: action === 'cancel' ? 'warning' : 'success' })
      await loadMigrations()
      pollRef.current?.()
    } catch (cause) {
      notice.add({ title: '存储任务操作失败', description: getAdminApiErrorMessage(cause), color: 'error' })
    } finally {
      setIsStorageTaskAction(false)
    }
  }

  const runThumbnailTaskAction = async (action: 'start' | 'cancel' | 'resume') => {
    if (isThumbnailTaskAction) return
    setIsThumbnailTaskAction(true)
    try {
      const endpoint = action === 'start'
        ? '/api/thumbnails/rebuilds'
        : `/api/thumbnails/rebuilds/${latestThumbnailJob?.id}/${action}`
      await adminApi.adminFetch(endpoint, { method: 'POST' })
      notice.add({
        title: action === 'start'
          ? '三层派生图开始重建'
          : action === 'resume' ? '派生图重建已继续' : '已请求安全中断',
        description: action === 'cancel'
          ? '正在停止尚未开始的项目，已完成的派生图会保留。'
          : '任务在后端并发运行，可以离开此页面。每张图片会生成完整三层。',
        color: action === 'cancel' ? 'warning' : 'success',
      })
      await loadThumbnailJob()
      pollRef.current?.()
    } catch (cause) {
      notice.add({
        title: '派生图任务操作失败',
        description: getAdminApiErrorMessage(cause),
        color: 'error',
      })
    } finally {
      setIsThumbnailTaskAction(false)
    }
  }

  const runS3CleanupAction = async (action: 'scan' | 'delete' | 'cancel' | 'resume') => {
    if (isS3CleanupAction) return
    // 确认放在置标志之前：用户在确认框里取消时按钮不应该转圈。
    if (action === 'delete') {
      const job = latestS3Cleanup
      const ok = await notice.confirm(
        `确定删除扫描到的 ${job?.total ?? 0} 个 S3 旧对象吗？\n\n`
        + `预计释放 ${formatBytes(job?.bytesFound ?? 0)}。只会处理 ${job?.managedPrefix ?? ''}，`
        + '删除前还会重新核对数据库引用；删除不能撤销。',
        true,
      )
      if (!ok) return
    }
    setIsS3CleanupAction(true)
    try {
      const endpoint = action === 'scan'
        ? '/api/s3-cleanups/scan'
        : `/api/s3-cleanups/${latestS3Cleanup?.id}/${action}`
      await adminApi.adminFetch(endpoint, { method: 'POST' })
      const titles: Record<typeof action, string> = {
        scan: 'S3 旧空间扫描已开始',
        delete: 'S3 旧对象开始清理',
        resume: 'S3 任务已继续',
        cancel: '已请求安全中断',
      }
      const descriptions: Record<typeof action, string | undefined> = {
        scan: '只扫描 Open Gallery 管理前缀；24 小时内的新对象不会进入清理清单。',
        delete: '任务在后端以 8 并发运行，可以离开此页面。',
        resume: undefined,
        cancel: undefined,
      }
      notice.add({
        title: titles[action],
        description: descriptions[action],
        color: action === 'cancel' ? 'warning' : 'success',
      })
      await loadS3Cleanup()
      pollRef.current?.()
    } catch (cause) {
      notice.add({
        title: 'S3 空间任务操作失败',
        description: getAdminApiErrorMessage(cause),
        color: 'error',
      })
    } finally {
      setIsS3CleanupAction(false)
    }
  }

  /* ------------------------------ 生命周期 ------------------------------ */

  const reloadRef = useRef(loadSettings)
  reloadRef.current = loadSettings

  useEffect(() => {
    const generation = pollGeneration.current + 1
    pollGeneration.current = generation
    mounted.current = true
    const alive = () => mounted.current && pollGeneration.current === generation

    void (async () => {
      // 必须先拿到 savedSignature 才能正确计算 isDirty，所以这一步是串行的。
      await reloadRef.current()
      await Promise.all([loadMigrations(), loadThumbnailJob(), loadS3Cleanup()])
      if (!alive()) return

      const poll = async () => {
        if (!alive()) return
        // 「忙 → 闲」只能比较刚取回的值：渲染期的布尔值在 await 之后仍是旧的，
        // 用它判断会让条件永远不成立，迁移结束后设置就一直停在脏状态。
        const before = isBusyState(migrationsRef.current, cleanupRef.current, thumbnailRef.current)
        await Promise.all([loadMigrations(), loadThumbnailJob(), loadS3Cleanup()])
        if (!alive()) return
        const after = isBusyState(migrationsRef.current, cleanupRef.current, thumbnailRef.current)

        // 存储任务结束时重载设置：这是发起迁移后清掉脏标记的唯一自动路径。
        if (before.storage && !after.storage) await reloadRef.current(true)
        if (!alive()) return

        const busy = after.storage || after.thumbnail
        timer.current = setTimeout(
          () => void poll(),
          document.hidden ? 60_000 : busy ? 5_000 : 30_000,
        )
      }

      const start = isBusyState(migrationsRef.current, cleanupRef.current, thumbnailRef.current)
      pollRef.current = () => {
        clearTimeout(timer.current)
        void poll()
      }
      timer.current = setTimeout(
        () => void poll(),
        (start.storage || start.thumbnail) ? 5_000 : 30_000,
      )
    })()

    return () => {
      mounted.current = false
      pollGeneration.current += 1
      pollRef.current = null
      clearTimeout(timer.current)
    }
  }, [loadMigrations, loadThumbnailJob, loadS3Cleanup])

  // 与相册页不同：这里用 replace，不产生历史条目；connection 不写进 URL。
  const setTab = (next: string) => {
    setSearchParams(next === 'connection' ? {} : { tab: next }, { replace: true })
  }

  const backendLabel = BACKENDS.find((item) => item.value === savedBackend)?.label ?? savedBackend
  // 比重写前**更严**：现网只禁用存储类型单选框，文本框始终可编辑。
  // 这里一并锁住是因为 applySettings 会整体覆盖表单——加载中或保存中输入的内容
  // 会被静默丢弃，锁住可以避免这种无声的数据丢失。storageBusy 期间也锁，与下方
  // 警告条「请完成或中断任务后再修改连接」的措辞一致（后端此时 PUT 也会返回 409）。
  const fieldDisabled = isLoading || isSaving || isTesting || storageBusy

  /* -------------------------------- 渲染 -------------------------------- */

  return (
    <div>
      <PageHeader
        title="存储与维护"
        description="原图存储配置保存在数据库中；迁移和清理任务在后台运行。"
        actions={
          <>
            <Chip color="success" variant="soft">{backendLabel}</Chip>
            <Button
              variant="secondary"
              isDisabled={isLoading}
              isPending={isLoading}
              onPress={() => {
                // 四个区块各自独立加载与反馈，互不阻塞（与现网一致）。
                void loadSettings()
                void loadMigrations()
                void loadThumbnailJob()
                void loadS3Cleanup()
              }}
            >
              <Icon name="refresh" size={16} />
              刷新
            </Button>
          </>
        }
      />

      {loadError ? (
        <div style={{ marginBottom: 20 }}>
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Description>
                <span className="preserve-newlines">{loadError}</span>
              </Alert.Description>
            </Alert.Content>
          </Alert>
        </div>
      ) : null}

      <Tabs selectedKey={tab} onSelectionChange={(key) => setTab(String(key))}>
        <Tabs.List>
          {TABS.map((item) => <Tabs.Tab key={item.id} id={item.id}>{item.label}</Tabs.Tab>)}
        </Tabs.List>
      </Tabs>

      <div style={{ marginTop: 20 }} />

      {/* ============================== 存储连接 ============================== */}
      {tab === 'connection' ? (
        <Card>
          <Card.Header>
            <Card.Title>原图存储配置</Card.Title>
          </Card.Header>
          <Card.Content>
            {lastTest ? (
              <div style={{ marginBottom: 20 }}>
                <Alert status="success">
                  <Alert.Indicator />
                  <Alert.Content>
                    <Alert.Title>连接测试通过</Alert.Title>
                    <Alert.Description>
                      {BACKENDS.find((item) => item.value === lastTest.backend)?.label ?? lastTest.backend}
                      {' · '}
                      {formatTime(lastTest.at)}
                    </Alert.Description>
                  </Alert.Content>
                </Alert>
              </div>
            ) : null}

            <form
              onSubmit={(event) => { event.preventDefault(); void saveSettings() }}
              style={{ maxWidth: 1060, display: 'flex', flexDirection: 'column', gap: 18 }}
            >
              <div>
                <div style={{ fontSize: 13, color: 'var(--muted)', marginBottom: 8 }}>存储类型</div>
                <RadioGroup
                  value={form.backend}
                  isDisabled={fieldDisabled}
                  onChange={(value) => {
                    // 切换后端只改表单，不发任何请求；已填的其它后端字段保留。
                    clearSensitiveInputs()
                    setLastTest(null)
                    setForm((previous) => ({ ...previous, backend: value as StorageBackend }))
                  }}
                >
                  {BACKENDS.map((item) => (
                    <Radio key={item.value} value={item.value}>{item.label}</Radio>
                  ))}
                </RadioGroup>
              </div>

              {form.backend === 'local' ? (
                <div>
                  <TextField
                    value={form.localPath}
                    isDisabled={fieldDisabled}
                    onChange={(localPath: string) => setForm((previous) => ({ ...previous, localPath }))}
                  >
                    <Label>本地存储路径</Label>
                    <Input />
                  </TextField>
                  <p className="admin-help" style={{ marginTop: 4 }}>
                    容器内路径，建议保持在 /app/data 下，随数据目录持久化。
                  </p>
                </div>
              ) : null}

              {form.backend === 'webdav' ? (
                <div className="admin-form-grid">
                  <TextField
                    value={form.webdavUrl}
                    isDisabled={fieldDisabled}
                    onChange={(webdavUrl: string) => setForm((previous) => ({ ...previous, webdavUrl }))}
                  >
                    <Label>WebDAV URL</Label>
                    <Input placeholder="https://dav.example.com" />
                  </TextField>
                  <TextField
                    value={form.webdavPrefix}
                    isDisabled={fieldDisabled}
                    onChange={(webdavPrefix: string) => setForm((previous) => ({ ...previous, webdavPrefix }))}
                  >
                    <Label>目录前缀</Label>
                    <Input />
                  </TextField>
                  <TextField
                    value={form.webdavUsername}
                    isDisabled={fieldDisabled}
                    onChange={(webdavUsername: string) =>
                      setForm((previous) => ({ ...previous, webdavUsername }))}
                  >
                    <Label>用户名</Label>
                    <Input autoComplete="off" />
                  </TextField>
                  <div>
                    <TextField
                      value={webdavPassword}
                      isDisabled={fieldDisabled}
                      onChange={setWebdavPassword}
                    >
                      <Label>密码</Label>
                      <Input type="password" autoComplete="new-password" />
                    </TextField>
                    <p className="admin-help" style={{ marginTop: 4 }}>
                      {webdavPasswordSet ? '已配置，留空保持不变。' : '首次使用请填写。'}
                    </p>
                  </div>
                </div>
              ) : null}

              {form.backend === 's3' ? (
                <div className="admin-form-grid">
                  <div>
                    <TextField
                      value={form.s3Endpoint}
                      isDisabled={fieldDisabled}
                      onChange={(s3Endpoint: string) => setForm((previous) => ({ ...previous, s3Endpoint }))}
                    >
                      <Label>S3 Endpoint</Label>
                      <Input placeholder="https://account-id.r2.cloudflarestorage.com" />
                    </TextField>
                    <p className="admin-help" style={{ marginTop: 4 }}>
                      R2 使用账户的 S3 API 地址，不包含桶名。
                    </p>
                  </div>
                  <div>
                    <TextField
                      value={form.s3Region}
                      isDisabled={fieldDisabled}
                      onChange={(s3Region: string) => setForm((previous) => ({ ...previous, s3Region }))}
                    >
                      <Label>区域</Label>
                      <Input />
                    </TextField>
                    <p className="admin-help" style={{ marginTop: 4 }}>Cloudflare R2 填 auto</p>
                  </div>
                  <TextField
                    value={form.s3Bucket}
                    isDisabled={fieldDisabled}
                    onChange={(s3Bucket: string) => setForm((previous) => ({ ...previous, s3Bucket }))}
                  >
                    <Label>桶名</Label>
                    <Input />
                  </TextField>
                  <div>
                    <TextField
                      value={form.s3Prefix}
                      isDisabled={fieldDisabled}
                      onChange={(s3Prefix: string) => setForm((previous) => ({ ...previous, s3Prefix }))}
                    >
                      <Label>存储前缀</Label>
                      <Input />
                    </TextField>
                    <p className="admin-help" style={{ marginTop: 4 }}>不要以 / 开头</p>
                  </div>
                  <TextField
                    value={form.s3AccessKey}
                    isDisabled={fieldDisabled}
                    onChange={(s3AccessKey: string) => setForm((previous) => ({ ...previous, s3AccessKey }))}
                  >
                    <Label>Access Key</Label>
                    <Input autoComplete="off" />
                  </TextField>
                  <div>
                    <TextField value={s3SecretKey} isDisabled={fieldDisabled} onChange={setS3SecretKey}>
                      <Label>Secret Key</Label>
                      <Input type="password" autoComplete="new-password" />
                    </TextField>
                    <p className="admin-help" style={{ marginTop: 4 }}>
                      {s3SecretKeySet ? '已配置，留空保持不变。' : '首次使用请填写。'}
                    </p>
                  </div>
                </div>
              ) : null}

              {storageBusy ? (
                <Alert status="warning">
                  <Alert.Indicator />
                  <Alert.Content>
                    <Alert.Description>
                      存储任务正在运行，请完成或中断任务后再修改连接。
                    </Alert.Description>
                  </Alert.Content>
                </Alert>
              ) : migrationRequired ? (
                <Alert status="accent">
                  <Alert.Indicator />
                  <Alert.Content>
                    <Alert.Description>
                      已存在 {storedPhotoCount} 张图片，将先复制校验，再切换到新存储。
                    </Alert.Description>
                  </Alert.Content>
                </Alert>
              ) : null}

              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
                <Button
                  type="submit"
                  variant="primary"
                  isDisabled={isTesting || !isDirty || storageBusy}
                  isPending={isSaving}
                >
                  {migrationRequired ? '开始安全迁移' : '保存并启用'}
                </Button>
                <Button
                  variant="secondary"
                  isDisabled={isSaving || storageBusy}
                  isPending={isTesting}
                  onPress={() => void testConnection()}
                >
                  测试连接
                </Button>
                <Button
                  variant="ghost"
                  // 「重置」就是重新 GET。首屏加载失败时 loaded 为 false、isDirty 恒为
                  // false，若只看 isDirty 就会把它锁死在最需要它的时刻，因此未加载
                  // 成功时保持可用，让用户能直接重试。
                  isDisabled={isSaving || (loaded && !isDirty)}
                  onPress={() => void loadSettings()}
                >
                  重置
                </Button>
              </div>

              <p className="admin-help">
                测试不会保存配置。密码和 Secret Key 发送后立即清空，不会写入浏览器存储。相册 ZIP
                与此处配置无关，始终保存在本地。
              </p>
            </form>
          </Card.Content>
        </Card>
      ) : null}

      {/* ============================== 存储迁移 ============================== */}
      {tab === 'migration' ? (
        <div className="admin-stack">
          {migrationLoadError ? (
            <Alert status="warning">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Description>{migrationLoadError}</Alert.Description>
              </Alert.Content>
            </Alert>
          ) : null}

          {!latestMigration ? (
            <Alert status="accent">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Description>
                  暂无迁移记录。修改存储连接并保存后，会自动创建迁移任务。
                </Alert.Description>
              </Alert.Content>
            </Alert>
          ) : (
            <Card>
              <Card.Header>
                <Card.Title>
                  <span style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                    最近一次迁移
                    <Chip color={migrationStatusTone(latestMigration)} variant="soft" size="sm">
                      {migrationStatusText(latestMigration)}
                    </Chip>
                  </span>
                </Card.Title>
              </Card.Header>
              <Card.Content>
                <div style={{ display: 'flex', gap: 32, flexWrap: 'wrap', marginBottom: 16 }}>
                  <Field label="来源" value={latestMigration.sourceBackend.toUpperCase()} />
                  <Field label="目标" value={latestMigration.targetBackend.toUpperCase()} />
                  <Field label="图片" value={String(latestMigration.total)} />
                </div>

                <ProgressBar
                  // 清理阶段改用 cleanupCompleted/total，且这里不夹取上限（与现网一致）。
                  value={latestMigration.cleanupStatus === 'cleaning'
                    ? (latestMigration.total
                      ? Math.round((latestMigration.cleanupCompleted / latestMigration.total) * 100)
                      : 0)
                    : progressPercent(latestMigration.completed, latestMigration.total)}
                  color="accent"
                  aria-label="迁移进度"
                >
                  <ProgressBar.Track><ProgressBar.Fill /></ProgressBar.Track>
                </ProgressBar>

                <p className="admin-help" style={{ marginTop: 8 }}>
                  成功 {latestMigration.succeeded} · 失败 {latestMigration.failed} · 旧对象已清理{' '}
                  {latestMigration.cleanupCompleted}
                </p>

                {latestMigration.error ? (
                  <div style={{ marginTop: 12 }}>
                    <Alert status="warning">
                      <Alert.Content>
                        <Alert.Description>
                          <span className="preserve-newlines">{latestMigration.error}</span>
                        </Alert.Description>
                      </Alert.Content>
                    </Alert>
                  </div>
                ) : null}

                {latestMigration.status === 'completed'
                  && ['pending', 'failed', 'interrupted'].includes(latestMigration.cleanupStatus) ? (
                    <div style={{ marginTop: 12 }}>
                      <Alert status="warning">
                        <Alert.Indicator />
                        <Alert.Content>
                          <Alert.Description>
                            新存储已启用，请决定是否删除旧存储中的副本。
                          </Alert.Description>
                        </Alert.Content>
                      </Alert>
                    </div>
                  ) : null}

                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginTop: 16 }}>
                  {['queued', 'running'].includes(latestMigration.status)
                    || latestMigration.cleanupStatus === 'cleaning' ? (
                      <Button
                        variant="secondary"
                        isPending={isStorageTaskAction}
                        onPress={() => void runStorageTaskAction(latestMigration, 'cancel')}
                      >
                        安全中断
                      </Button>
                    ) : null}
                  {['failed', 'cancelled', 'interrupted'].includes(latestMigration.status) ? (
                    <Button
                      variant="primary"
                      isPending={isStorageTaskAction}
                      onPress={() => void runStorageTaskAction(latestMigration, 'resume')}
                    >
                      继续迁移
                    </Button>
                  ) : null}
                  {latestMigration.status === 'completed'
                    && ['pending', 'failed', 'interrupted'].includes(latestMigration.cleanupStatus) ? (
                      <>
                        <Button
                          variant="danger"
                          isPending={isStorageTaskAction}
                          onPress={() => void runStorageTaskAction(latestMigration, 'cleanup')}
                        >
                          删除旧存储图片
                        </Button>
                        <Button
                          variant="secondary"
                          isPending={isStorageTaskAction}
                          onPress={() => void runStorageTaskAction(latestMigration, 'retain')}
                        >
                          保留旧副本
                        </Button>
                      </>
                    ) : null}
                </div>
              </Card.Content>
            </Card>
          )}
        </div>
      ) : null}

      {/* ============================== 图片缓存 ============================== */}
      {tab === 'cache' ? (
        <Card>
          <Card.Header>
            <Card.Title>
              <span style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                重建三层浏览图
                <Chip color={thumbnailStatusTone(latestThumbnailJob)} variant="soft" size="sm">
                  {thumbnailStatusText(latestThumbnailJob)}
                </Chip>
              </span>
            </Card.Title>
          </Card.Header>
          <Card.Content>
            <p className="admin-help">
              生成 320px PNG、≤1.5 MB WebP 预览和 ≤5 MB WebP 高清图。此操作不改变原图，也不删除下载 ZIP。
            </p>

            {thumbnailLoadError ? (
              <div style={{ marginTop: 12 }}>
                <Alert status="warning">
                  <Alert.Content>
                    <Alert.Description>{thumbnailLoadError}</Alert.Description>
                  </Alert.Content>
                </Alert>
              </div>
            ) : null}

            {latestThumbnailJob ? (
              <div style={{ marginTop: 16 }}>
                <ProgressBar
                  value={jobProgressPercent(
                    latestThumbnailJob.completed,
                    latestThumbnailJob.total,
                    latestThumbnailJob.status,
                  )}
                  color="accent"
                  aria-label="缓存重建进度"
                >
                  <ProgressBar.Track><ProgressBar.Fill /></ProgressBar.Track>
                </ProgressBar>
                <p className="admin-help" style={{ marginTop: 8 }}>
                  {latestThumbnailJob.completed} / {latestThumbnailJob.total} · 成功{' '}
                  {latestThumbnailJob.succeeded} · 失败 {latestThumbnailJob.failed} · 并发{' '}
                  {latestThumbnailJob.workerCount}
                </p>
                {latestThumbnailJob.error ? (
                  <p className="admin-field-error" style={{ marginTop: 8 }}>
                    {latestThumbnailJob.error}
                  </p>
                ) : null}
              </div>
            ) : null}

            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginTop: 16 }}>
              {thumbnailTaskActive ? (
                <Button
                  variant="secondary"
                  isPending={isThumbnailTaskAction}
                  onPress={() => void runThumbnailTaskAction('cancel')}
                >
                  安全中断
                </Button>
              ) : (
                <Button
                  variant="primary"
                  isDisabled={storageBusy}
                  isPending={isThumbnailTaskAction}
                  onPress={() => void runThumbnailTaskAction('start')}
                >
                  清空并重新生成
                </Button>
              )}
              {latestThumbnailJob
                && ['failed', 'cancelled', 'interrupted'].includes(latestThumbnailJob.status) ? (
                  <Button
                    variant="secondary"
                    isPending={isThumbnailTaskAction}
                    onPress={() => void runThumbnailTaskAction('resume')}
                  >
                    继续上次任务
                  </Button>
                ) : null}
            </div>
          </Card.Content>
        </Card>
      ) : null}

      {/* ============================ S3 空间清理 ============================ */}
      {tab === 'cleanup' ? (
        <Card>
          <Card.Header>
            <Card.Title>
              <span style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
                清理失去引用的旧对象
                <Chip color={s3CleanupStatusTone(latestS3Cleanup)} variant="soft" size="sm">
                  {s3CleanupStatusText(latestS3Cleanup)}
                </Chip>
              </span>
            </Card.Title>
          </Card.Header>
          <Card.Content>
            <Alert status="accent">
              <Alert.Indicator />
              <Alert.Content>
                <Alert.Title>先扫描，再由管理员确认删除</Alert.Title>
                <Alert.Description>
                  仅处理 Open Gallery 管理前缀，保护数据库引用和 24 小时内的新对象。不会删除本地 ZIP。
                </Alert.Description>
              </Alert.Content>
            </Alert>

            {s3CleanupLoadError ? (
              <div style={{ marginTop: 12 }}>
                <Alert status="warning">
                  <Alert.Content>
                    <Alert.Description>{s3CleanupLoadError}</Alert.Description>
                  </Alert.Content>
                </Alert>
              </div>
            ) : null}

            {latestS3Cleanup ? (
              <div style={{ marginTop: 16 }}>
                <div style={{ display: 'flex', gap: 32, flexWrap: 'wrap', marginBottom: 16 }}>
                  <Field label="已扫描对象" value={String(latestS3Cleanup.scannedObjects)} />
                  <Field label="候选旧对象" value={String(latestS3Cleanup.total)} />
                  <Field label="预计释放" value={formatBytes(latestS3Cleanup.bytesFound)} />
                </div>
                <ProgressBar
                  value={jobProgressPercent(
                    latestS3Cleanup.completed,
                    latestS3Cleanup.total,
                    latestS3Cleanup.status,
                  )}
                  color="accent"
                  aria-label="S3 清理进度"
                >
                  <ProgressBar.Track><ProgressBar.Fill /></ProgressBar.Track>
                </ProgressBar>
                <p className="admin-help" style={{ marginTop: 8 }}>
                  已删除 {latestS3Cleanup.deleted} · 已释放 {formatBytes(latestS3Cleanup.bytesDeleted)} ·
                  失败 {latestS3Cleanup.failed}
                </p>
                {latestS3Cleanup.error ? (
                  <p className="admin-field-error" style={{ marginTop: 8 }}>{latestS3Cleanup.error}</p>
                ) : null}
              </div>
            ) : null}

            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginTop: 16 }}>
              {s3CleanupActive ? (
                <Button
                  variant="secondary"
                  isPending={isS3CleanupAction}
                  onPress={() => void runS3CleanupAction('cancel')}
                >
                  安全中断
                </Button>
              ) : (
                <Button
                  variant="primary"
                  // 现网用的是「已保存的后端」而不是表单里当前选中的后端。
                  isDisabled={savedBackend !== 's3' || storageBusy}
                  isPending={isS3CleanupAction}
                  onPress={() => void runS3CleanupAction('scan')}
                >
                  扫描旧对象
                </Button>
              )}
              {latestS3Cleanup?.status === 'ready' && latestS3Cleanup.total > 0 ? (
                <Button
                  variant="danger"
                  isPending={isS3CleanupAction}
                  onPress={() => void runS3CleanupAction('delete')}
                >
                  确认删除旧对象
                </Button>
              ) : null}
              {latestS3Cleanup
                && ['failed', 'cancelled', 'interrupted'].includes(latestS3Cleanup.status) ? (
                  <Button
                    variant="secondary"
                    isPending={isS3CleanupAction}
                    onPress={() => void runS3CleanupAction('resume')}
                  >
                    继续任务
                  </Button>
                ) : null}
            </div>
          </Card.Content>
        </Card>
      ) : null}
    </div>
  )
}

function Field({ label, value }: { label: string, value: string }) {
  return (
    <div>
      <div style={{ fontSize: 13, color: 'var(--muted)' }}>{label}</div>
      <div style={{ marginTop: 4, fontSize: 18, fontWeight: 600 }}>{value}</div>
    </div>
  )
}
