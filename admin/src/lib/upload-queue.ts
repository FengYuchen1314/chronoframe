// 上传队列核心逻辑。
// 逐字移植自 Vue 侧的 shared/utils/admin-upload-queue.ts（框架无关的纯函数）。
//
// 三条必须保留的语义，改错任何一条都会破坏现网行为：
//   1. 单一共享控制器，7 个名额跨所有相册共享。
//   2. 绝不 abort 已发出的上传——丢了响应就无法判断服务端是否已经提交。
//   3. pump() 里"扣名额"与"置为 uploading"必须同步完成，否则并发会超过 7。
//   4. 每次状态变化后必须调用 notify()。调用方（React store）靠它触发重渲染，
//      而状态变化有一半发生在 Promise 回调里——只在动作函数里通知是不够的：
//      'uploading' → 'done'/'failed' 这一步没有任何外部动作可以挂钩。

export type UploadStatus = 'queued' | 'uploading' | 'done' | 'failed'

export interface UploadItem<T> {
  id: number
  albumId: string
  albumName: string
  name: string
  size: number
  file?: T
  status: UploadStatus
  error: string
}

export interface UploadQueueState<T> {
  items: UploadItem<T>[]
  paused: boolean
  nextId: number
  /** albumId -> 成功上传计数，用于让相册详情页触发局部刷新。 */
  albumVersions: Record<string, number>
}

export function createUploadQueueState<T>(): UploadQueueState<T> {
  return { items: [], paused: false, nextId: 1, albumVersions: {} }
}

export interface UploadQueue<T> {
  enqueue: (files: T[], album: { id: string, name: string }) => void
  pause: () => void
  resume: () => void
  retryFailed: () => void
  remove: (id: number) => void
  clearDone: () => void
}

export function createUploadQueue<T extends { name: string, size: number }>(
  state: UploadQueueState<T>,
  upload: (file: T, albumId: string) => Promise<unknown>,
  describeError: (error: unknown) => string,
  /** 状态变化后的通知回调。默认空实现，纯逻辑测试无需传。 */
  notify: () => void = () => {},
  concurrency = 7,
): UploadQueue<T> {
  const pump = () => {
    if (state.paused) return
    let slots = concurrency - state.items.filter((item) => item.status === 'uploading').length
    let started = 0
    for (const item of state.items) {
      if (slots <= 0) break
      if (item.status !== 'queued' || !item.file) continue
      slots -= 1
      // 同步占位：必须在同一个同步循环里完成，否则下一次 pump 会重复启动同一项。
      item.status = 'uploading'
      started += 1
      const file = item.file
      void Promise.resolve()
        .then(() => upload(file, item.albumId))
        .then(() => {
          item.status = 'done'
          // 成功后释放浏览器的 File 引用，避免长期占用内存。
          item.file = undefined
          state.albumVersions[item.albumId] = (state.albumVersions[item.albumId] || 0) + 1
        })
        .catch((cause: unknown) => {
          item.status = 'failed'
          item.error = describeError(cause)
        })
        // 终态已经写入，先通知再 pump：让 UI 看到这一项的 done/failed，
        // 也让 albumVersions 的递增被读到（相册详情页的自动刷新依赖它）。
        .finally(() => {
          notify()
          pump()
        })
    }
    // 循环外统一通知：循环内逐项通知会在"扣名额"的同步段里触发重渲染。
    if (started > 0) notify()
  }

  return {
    enqueue(files, album) {
      for (const file of files) {
        state.items.push({
          id: state.nextId++,
          albumId: album.id,
          albumName: album.name,
          name: file.name,
          size: file.size,
          file,
          status: 'queued',
          error: '',
        })
      }
      // pump() 内部只在真的启动了任务时通知；入队本身也要让 UI 看到排队项。
      pump()
      notify()
    },
    pause() { state.paused = true; notify() },
    // 恢复必须同时 pump：pump 只在 enqueue/resume/retryFailed/任务完成时被调用，
    // 只置 paused=false 而不 pump，队列会永久卡住。
    resume() { state.paused = false; pump(); notify() },
    retryFailed() {
      for (const item of state.items) {
        if (item.status === 'failed' && item.file) {
          item.status = 'queued'
          item.error = ''
        }
      }
      pump()
      notify()
    },
    remove(id) {
      const index = state.items.findIndex((item) => item.id === id)
      if (index >= 0 && state.items[index]?.status !== 'uploading') state.items.splice(index, 1)
      notify()
    },
    clearDone() {
      state.items = state.items.filter((item) => item.status !== 'done')
      notify()
    },
  }
}
