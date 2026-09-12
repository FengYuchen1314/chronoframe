import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Alert, Button, Chip, Drawer, ProgressBar } from '@heroui/react'
import { useUploads } from '../lib/store'
import { notice } from '../lib/notice'
import { uploadStatusText, uploadStatusTone, adminBytes } from '../lib/format'
import { Icon } from '../lib/icons'

/**
 * 上传队列抽屉。对应 Vue 侧的 app/components/dashboard/UploadQueue.vue。
 *
 * 抽屉开关本身是全局状态，因此相册页、任务中心、退出登录拦截都能从外部打开它。
 */
export default function UploadQueueDrawer() {
  const uploads = useUploads()
  const navigate = useNavigate()
  const { items, active, queued, done, failed, pending, state, queue, open, setOpen } = uploads

  // 与现网一致：队列里还有未结束或未确认的项时，离开/刷新页面给出浏览器原生确认。
  useEffect(() => {
    const handler = (event: BeforeUnloadEvent) => {
      if (pending || failed) {
        event.preventDefault()
        event.returnValue = ''
      }
    }
    window.addEventListener('beforeunload', handler)
    return () => window.removeEventListener('beforeunload', handler)
  }, [pending, failed])

  const retry = async () => {
    // 现网确认文案：failed 的语义是「没拿到成功响应」，不代表服务端一定没入库。
    const ok = await notice.confirm(
      '失败请求可能已经入库，请先核对对应相册，避免重复上传。确认重新上传所有失败文件？',
    )
    if (ok) queue.retryFailed()
  }

  const totalPercent = items.length ? Math.round((done / items.length) * 100) : 0

  return (
    <Drawer.Root isOpen={open} onOpenChange={(next: boolean) => setOpen(next)}>
      <Drawer.Backdrop>
        <Drawer.Content placement="right" className="w-[min(720px,100vw)]">
          <Drawer.Dialog>
            <Drawer.Header>
              <Drawer.Heading>上传队列</Drawer.Heading>
            </Drawer.Header>

            <Drawer.Body>
              <Alert status="accent">
                <Alert.Content>
                  <Alert.Title>可切换后台页面，上传会继续</Alert.Title>
                  <Alert.Description>
                    7 个并发任务，文件始终上传到选择时的相册。不要刷新或关闭浏览器；离线时队列不会自动重试。
                  </Alert.Description>
                </Alert.Content>
              </Alert>

              <div
                style={{
                  display: 'flex',
                  flexWrap: 'wrap',
                  gap: 16,
                  margin: '20px 0 8px',
                  fontSize: 14,
                }}
              >
                <span>已入库 {done}</span>
                <span>处理中 {active}</span>
                <span>待上传 {queued}</span>
                {failed ? <span style={{ color: 'var(--danger)' }}>未确认 {failed}</span> : null}
              </div>

              {items.length ? (
                <ProgressBar
                  value={totalPercent}
                  color={failed ? 'danger' : 'accent'}
                  aria-label="上传总进度"
                >
                  <ProgressBar.Track>
                    <ProgressBar.Fill />
                  </ProgressBar.Track>
                </ProgressBar>
              ) : null}

              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, margin: '16px 0' }}>
                {pending ? (
                  <Button
                    variant="secondary"
                    size="sm"
                    onPress={() => (state.paused ? queue.resume() : queue.pause())}
                  >
                    <Icon name={state.paused ? 'player-play' : 'player-pause'} size={16} />
                    {state.paused ? '继续上传' : '暂停队列'}
                  </Button>
                ) : null}
                {failed ? (
                  <Button variant="secondary" size="sm" onPress={() => void retry()}>
                    <Icon name="refresh" size={16} />
                    重试失败文件
                  </Button>
                ) : null}
                <Button
                  variant="ghost"
                  size="sm"
                  isDisabled={!done}
                  onPress={() => queue.clearDone()}
                >
                  清除已完成记录
                </Button>
              </div>

              {state.paused ? (
                <Alert status="warning">
                  <Alert.Content>
                    <Alert.Description>已暂停新任务，正在上传的文件会继续完成</Alert.Description>
                  </Alert.Content>
                </Alert>
              ) : null}

              <div style={{ marginTop: 16 }}>
                {items.length === 0 ? (
                  <p className="admin-help" style={{ padding: '24px 0', textAlign: 'center' }}>
                    队列为空
                  </p>
                ) : (
                  <ul style={{ listStyle: 'none', margin: 0, padding: 0 }}>
                    {items.map((item) => (
                      <li
                        key={item.id}
                        style={{
                          display: 'flex',
                          alignItems: 'flex-start',
                          gap: 12,
                          padding: '10px 0',
                          borderBottom: '1px solid var(--separator)',
                        }}
                      >
                        <div style={{ flex: 1, minWidth: 0 }}>
                          <div style={{ overflowWrap: 'anywhere', fontSize: 14 }}>{item.name}</div>
                          <div
                            style={{ display: 'flex', alignItems: 'center', gap: 6, marginTop: 2 }}
                          >
                            <button
                              type="button"
                              className="admin-help"
                              style={{
                                background: 'none',
                                border: 0,
                                padding: 0,
                                cursor: 'pointer',
                                color: 'var(--link)',
                              }}
                              onClick={() => {
                                setOpen(false)
                                navigate(`/albums?album=${encodeURIComponent(item.albumId)}`)
                              }}
                            >
                              {item.albumName}
                            </button>
                            <span className="admin-help">· {adminBytes(item.size)}</span>
                          </div>
                          {item.error ? (
                            <p className="admin-field-error" style={{ marginTop: 4 }}>
                              {item.error}
                            </p>
                          ) : null}
                        </div>

                        <Chip
                          color={uploadStatusTone[item.status] ?? 'default'}
                          variant="soft"
                          size="sm"
                        >
                          {uploadStatusText[item.status] ?? item.status}
                        </Chip>

                        <Button
                          variant="ghost"
                          size="sm"
                          isDisabled={item.status === 'uploading'}
                          onPress={() => queue.remove(item.id)}
                        >
                          {item.status === 'queued' ? '取消' : '移除'}
                        </Button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>

              <p className="admin-help" style={{ marginTop: 16 }}>
                取消仅移除尚未开始的队列项；移除记录和清除记录均不会删除已入库的图片。
              </p>
            </Drawer.Body>
          </Drawer.Dialog>
        </Drawer.Content>
      </Drawer.Backdrop>
    </Drawer.Root>
  )
}
