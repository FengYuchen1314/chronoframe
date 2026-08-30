<script setup lang="ts">
import type { PublicAlbumDownload } from '~~/shared/types/downloads'
import { isTouchAlbumDownloadDevice } from '~~/shared/utils/albumPhotoDownload'
const props = defineProps<{ download?: PublicAlbumDownload }>()
const mounted = useMounted()
const coarsePointer = useMediaQuery('(pointer: coarse)')
const narrow = useMediaQuery('(max-width: 767px)')
const touchDevice = computed(() => mounted.value && isTouchAlbumDownloadDevice(navigator.userAgent, navigator.maxTouchPoints, coarsePointer.value, narrow.value))
const dialogLoaded = ref(false)
const dialogOpen = ref(false)
const openPhotos = () => { dialogLoaded.value = true; dialogOpen.value = true }
const save = (url?: string | null) => {
  if (!url) return
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = ''
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
}
const label = (status: string) => ['queued', 'running'].includes(status) ? '正在打包' : '暂不可用'
const items = computed(() => (props.download?.formats || []).map(item => ({
  label: `${item.format.toUpperCase()} ZIP${item.url ? '' : ` · ${label(item.status)}`}`,
  icon: 'tabler:file-zip', disabled: !item.url, onSelect: () => save(item.url),
})))
</script>
<template>
  <div v-if="download?.formats.length" @click.stop @pointerdown.stop>
    <UButton v-if="touchDevice" icon="tabler:download" color="neutral" variant="ghost" class="album-download-button" @click="openPhotos">下载图片</UButton>
    <UDropdownMenu v-else-if="download.formats.length > 1" :items="items" :content="{ align: 'end' }"><UButton icon="tabler:download" trailing-icon="tabler:chevron-down" color="neutral" variant="ghost" class="album-download-button" aria-label="选择相册 ZIP 下载格式">下载相册</UButton></UDropdownMenu>
    <UButton v-else icon="tabler:download" color="neutral" variant="ghost" class="album-download-button" :disabled="!download.formats[0]?.url" @click="save(download.formats[0]?.url)">{{ download.formats[0]?.url ? '下载相册' : label(download.formats[0]?.status || '') }}</UButton>
    <LazyAlbumPhotoDownloadDialog v-if="dialogLoaded" v-model:open="dialogOpen" :download="download" />
  </div>
</template>

<style scoped>
.album-download-button {
  min-height: 40px;
  color: #fff;
  background: rgb(12 12 14 / 58%);
  border: 1px solid rgb(255 255 255 / 18%);
  border-radius: 10px;
  -webkit-backdrop-filter: blur(16px) saturate(140%);
  backdrop-filter: blur(16px) saturate(140%);
  box-shadow: 0 2px 12px rgb(0 0 0 / 12%), inset 0 1px 0 rgb(255 255 255 / 6%);
  transition: background-color 160ms ease, border-color 160ms ease;
}
.album-download-button:hover:not(:disabled),
.album-download-button[data-state='open'] { background: rgb(12 12 14 / 74%); border-color: rgb(255 255 255 / 30%); }
.album-download-button:active:not(:disabled) { background: rgb(12 12 14 / 82%); }
.album-download-button:focus-visible { outline: 2px solid #fff; outline-offset: 3px; box-shadow: 0 0 0 5px rgb(0 0 0 / 60%); }
.album-download-button:disabled { color: rgb(255 255 255 / 70%); opacity: 1; cursor: not-allowed; }
@media (prefers-reduced-motion: reduce) { .album-download-button { transition: none; } }
</style>
