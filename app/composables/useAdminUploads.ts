import { createUploadQueue, createUploadQueueState } from '~~/shared/utils/admin-upload-queue'

const controllers = new WeakMap<object, ReturnType<typeof createUploadQueue<File>>>()

export function useAdminUploads() {
  const { adminFetch } = useAdminApi()
  const state = useState('admin-upload-queue', () => createUploadQueueState<File>())
  const open = useState('admin-upload-queue-open', () => false)
  let controller = controllers.get(state.value)
  if (!controller) {
    controller = createUploadQueue(state.value, async (file, albumId) => {
      const body = new FormData()
      body.append('files', file)
      await adminFetch(`/api/albums/${encodeURIComponent(albumId)}/photos`, { method: 'POST', body })
    }, getAdminApiErrorMessage)
    controllers.set(state.value, controller)
  }
  const active = computed(() => state.value.items.filter(item => item.status === 'uploading').length)
  const queued = computed(() => state.value.items.filter(item => item.status === 'queued').length)
  const failed = computed(() => state.value.items.filter(item => item.status === 'failed').length)
  const done = computed(() => state.value.items.filter(item => item.status === 'done').length)
  const pending = computed(() => active.value + queued.value)
  return { state, open, active, queued, failed, done, pending, ...controller }
}
