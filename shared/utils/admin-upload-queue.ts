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
  albumVersions: Record<string, number>
}

export function createUploadQueueState<T>(): UploadQueueState<T> {
  return { items: [], paused: false, nextId: 1, albumVersions: {} }
}

// One shared controller, seven slots across ALL albums. Never abort an upload:
// a lost response cannot tell us whether the server has already committed it.
export function createUploadQueue<T extends { name: string, size: number }>(
  state: UploadQueueState<T>,
  upload: (file: T, albumId: string) => Promise<unknown>,
  describeError: (error: unknown) => string,
  concurrency = 7,
) {
  const pump = () => {
    if (state.paused) return
    let slots = concurrency - state.items.filter(item => item.status === 'uploading').length
    for (const item of state.items) {
      if (slots <= 0) break
      if (item.status !== 'queued' || !item.file) continue
      slots--
      item.status = 'uploading'
      const file = item.file
      void Promise.resolve().then(() => upload(file, item.albumId)).then(() => {
        item.status = 'done'
        item.file = undefined // Release the browser's File reference after success.
        state.albumVersions[item.albumId] = (state.albumVersions[item.albumId] || 0) + 1
      }).catch((cause: unknown) => {
        item.status = 'failed'
        item.error = describeError(cause)
      }).finally(pump)
    }
  }
  return {
    enqueue(files: T[], album: { id: string, name: string }) {
      for (const file of files) state.items.push({
        id: state.nextId++, albumId: album.id, albumName: album.name,
        name: file.name, size: file.size, file, status: 'queued', error: '',
      })
      pump()
    },
    pause() { state.paused = true },
    resume() { state.paused = false; pump() },
    retryFailed() {
      for (const item of state.items) {
        if (item.status === 'failed' && item.file) { item.status = 'queued'; item.error = '' }
      }
      pump()
    },
    remove(id: number) {
      const index = state.items.findIndex(item => item.id === id)
      if (index >= 0 && state.items[index]?.status !== 'uploading') state.items.splice(index, 1)
    },
    clearDone() { state.items = state.items.filter(item => item.status !== 'done') },
  }
}
