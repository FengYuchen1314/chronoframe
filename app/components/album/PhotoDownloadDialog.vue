<script setup lang="ts">
import type { AlbumPhotoDownloadList, DownloadFormat, PublicAlbumDownload } from '~~/shared/types/downloads'
import { downloadAlbumSequence } from '~~/shared/utils/albumPhotoDownload'
import { saveAlbumPhoto } from '~/utils/saveAlbumPhoto'

const props = defineProps<{ open: boolean; download: PublicAlbumDownload }>()
const emit = defineEmits<{ 'update:open': [boolean] }>()
const format = ref<DownloadFormat>('webp')
const groupName = useId()
const manifest = ref<AlbumPhotoDownloadList>()
const loading = ref(false)
const running = ref(false)
const completed = ref(0)
const stopped = ref(false)
const error = ref('')
const selected = computed(() => props.download.formats.find(item => item.format === format.value))
const total = computed(() => manifest.value?.photos.length || 0)
const finished = computed(() => total.value > 0 && completed.value === total.value)
const progress = computed(() => total.value ? Math.round(completed.value / total.value * 100) : 0)
let metadataRequest: AbortController | undefined
let transferRequest: AbortController | undefined

const fetchFile = async (url: string, signal: AbortSignal) => {
  const controller = new AbortController()
  const abort = () => controller.abort()
  signal.addEventListener('abort', abort, { once: true })
  if (signal.aborted) controller.abort()
  const timer = window.setTimeout(abort, 90_000)
  try {
    const response = await fetch(url, { signal: controller.signal, credentials: 'same-origin' })
    if (!response.ok) throw new Error(response.status === 404 ? '图片版本已更新或下载已关闭，请关闭窗口后重新打开。' : `请求失败（${response.status}），可重试剩余图片。`)
    if (!response.headers.get('content-type')?.startsWith('image/')) throw new Error('图片响应无效，请稍后重试。')
    return await response.blob()
  } catch (cause) {
    if (controller.signal.aborted && !signal.aborted) throw new Error('图片下载超时，可继续重试剩余图片。')
    throw cause
  } finally { clearTimeout(timer); signal.removeEventListener('abort', abort) }
}

const loadManifest = async () => {
  metadataRequest?.abort()
  transferRequest?.abort()
  manifest.value = undefined
  completed.value = 0
  stopped.value = false
  error.value = ''
  loading.value = false
  if (!props.open || !selected.value?.photosUrl) return
  const controller = new AbortController()
  metadataRequest = controller
  loading.value = true
  try {
    const result = await $fetch<AlbumPhotoDownloadList>(selected.value.photosUrl, { signal: controller.signal, timeout: 30_000, retry: 0 })
    if (!controller.signal.aborted && metadataRequest === controller) manifest.value = result
  } catch (cause) {
    if (!controller.signal.aborted && metadataRequest === controller) error.value = '图片列表加载失败，请重试；如果压缩包已被撤下，请联系管理员。'
  } finally { if (metadataRequest === controller) loading.value = false }
}

watch(() => props.open, (open) => {
  if (open) {
    const choices: DownloadFormat[] = ['jpg', 'jpeg', 'png', 'webp']
    format.value = choices.find(value => props.download.formats.some(item => item.format === value && item.photosUrl)) || props.download.formats[0]?.format || 'webp'
  } else { metadataRequest?.abort(); transferRequest?.abort() }
}, { immediate: true })
watch([() => props.open, () => selected.value?.photosUrl], () => { void loadManifest() }, { immediate: true })

const start = async () => {
  if (running.value || !manifest.value || !total.value || finished.value) return
  const controller = new AbortController()
  transferRequest = controller
  running.value = true
  stopped.value = false
  error.value = ''
  try {
    await downloadAlbumSequence(manifest.value.photos, {
      start: completed.value,
      signal: controller.signal,
      load: async (photo, signal) => {
        const blob = await fetchFile(photo.url, signal)
        if (blob.size !== photo.byteSize) throw new Error('图片未完整接收，请重试剩余图片。')
        return blob
      },
      save: (blob, photo) => saveAlbumPhoto(blob, photo.name),
      progress: count => { completed.value = count },
      pause: () => new Promise(resolve => window.setTimeout(resolve, 250)),
    })
  } catch (cause) {
    if (controller.signal.aborted) stopped.value = true
    else error.value = cause instanceof Error ? cause.message : '下载中断，可重试剩余图片。'
  } finally { if (transferRequest === controller) running.value = false }
}
const close = (open: boolean) => { if (!open) { metadataRequest?.abort(); transferRequest?.abort() }; emit('update:open', open) }
onBeforeUnmount(() => { metadataRequest?.abort(); transferRequest?.abort() })
</script>

<template>
  <UModal :open="open" :dismissible="!running" :close="!running" title="下载所有照片到相册？" :description="manifest ? `${manifest.albumName} · 共 ${total} 张照片` : '下载此相册的全部照片'" :ui="{ content: 'album-photo-download-dialog max-w-md', overlay: 'bg-black/30 backdrop-blur-sm', header: 'border-white/10', title: 'text-white', description: 'text-white/65', body: 'space-y-4', footer: 'border-white/10 grid grid-cols-2', close: 'text-white/65 hover:bg-white/10' }" @update:open="close">
    <template #body>
      <div v-if="download.formats.length > 1" role="radiogroup" aria-label="下载图片格式" class="flex flex-wrap gap-2">
        <label v-for="item in download.formats" :key="item.format" class="relative">
          <input v-model="format" type="radio" :name="groupName" :value="item.format" :disabled="running || !item.photosUrl" class="peer sr-only" />
          <span class="block rounded-xl border border-white/15 bg-white/5 px-4 py-2.5 text-sm text-white/65 transition peer-checked:border-white/60 peer-checked:bg-white/20 peer-checked:text-white peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-white peer-disabled:opacity-35">{{ item.format.toUpperCase() }}</span>
        </label>
      </div>
      <p class="text-sm leading-relaxed text-white/75">确认后将按顺序下载所有照片，不下载压缩包。图片格式和大小沿用管理员设置。</p>
      <p class="rounded-2xl border border-white/10 bg-white/5 p-3 text-xs leading-relaxed text-white/60">浏览器无法直接写入系统相册。若照片未出现在相册，请在“下载”或“文件”中选中图片，再选择“存储图像 / 保存到相册”。请允许多文件下载，并保持此页面打开。</p>
      <p v-if="loading" role="status" class="text-sm text-white/70">正在读取图片列表…</p>
      <p v-else-if="!selected?.photosUrl" role="status" class="text-sm text-white/70">图片尚未准备好，或压缩包已被撤下，请稍后再试。</p>
      <p v-else-if="manifest && !total" role="status" class="text-sm text-white/70">此相册还没有照片。</p>
      <div v-if="running || completed || stopped" class="space-y-2">
        <div class="flex justify-between gap-2 text-sm" aria-live="polite"><span>{{ finished ? '下载请求已全部发出' : stopped ? '已停止，可继续剩余图片' : '已交给浏览器下载' }}</span><span>{{ completed }} / {{ total }}</span></div>
        <div role="progressbar" aria-label="照片下载进度" :aria-valuenow="completed" :aria-valuemax="total" aria-valuemin="0" class="h-1.5 overflow-hidden rounded-full bg-white/15"><div class="h-full rounded-full bg-white/85 transition-[width] duration-200 motion-reduce:transition-none" :style="{ width: `${progress}%` }" /></div>
      </div>
      <p v-if="error" role="alert" class="rounded-xl border border-red-300/20 bg-red-400/10 p-3 text-sm text-red-200">{{ error }}</p>
      <button v-if="error && !manifest" type="button" class="text-sm text-white underline underline-offset-4" @click="loadManifest">重试加载列表</button>
    </template>
    <template #footer>
      <button type="button" class="photo-download-secondary" :disabled="running" @click="close(false)">{{ completed || stopped ? '关闭' : '取消' }}</button>
      <button v-if="running" type="button" class="photo-download-primary" @click="transferRequest?.abort()">停止下载</button>
      <button v-else type="button" class="photo-download-primary" :disabled="loading || !total || finished" @click="start">{{ finished ? '已发出下载' : completed ? '继续下载剩余图片' : '确认下载' }}</button>
    </template>
  </UModal>
</template>

<style scoped>
:global(.album-photo-download-dialog) {
  color: #fff;
  background: rgb(12 12 14 / 76%);
  -webkit-backdrop-filter: blur(24px) saturate(140%);
  backdrop-filter: blur(24px) saturate(140%);
  border: 1px solid rgb(255 255 255 / 18%);
  border-radius: 24px;
  box-shadow: 0 24px 80px rgb(0 0 0 / 35%), inset 0 1px 0 rgb(255 255 255 / 6%);
}
.photo-download-primary, .photo-download-secondary { min-height: 44px; border-radius: 12px; padding: 10px 12px; font-size: 14px; font-weight: 600; cursor: pointer; transition: background-color 150ms ease; }
.photo-download-primary { background: rgb(255 255 255 / 90%); color: #18181b; }
.photo-download-primary:hover:not(:disabled) { background: #fff; }
.photo-download-secondary { background: rgb(255 255 255 / 8%); color: rgb(255 255 255 / 85%); border: 1px solid rgb(255 255 255 / 12%); }
.photo-download-secondary:hover:not(:disabled) { background: rgb(255 255 255 / 15%); }
.photo-download-primary:disabled, .photo-download-secondary:disabled { opacity: .4; cursor: not-allowed; }
.photo-download-primary:focus-visible, .photo-download-secondary:focus-visible { outline: 2px solid white; outline-offset: 3px; }
@media (prefers-reduced-motion: reduce) { .photo-download-primary, .photo-download-secondary { transition: none; } }
</style>
