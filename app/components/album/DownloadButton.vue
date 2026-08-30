<script setup lang="ts">
import type { PublicAlbumDownload } from '~~/shared/types/downloads'
const props = defineProps<{ download?: PublicAlbumDownload }>()
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
    <UDropdownMenu v-if="download.formats.length > 1" :items="items" :content="{ align: 'end' }"><UButton icon="tabler:download" trailing-icon="tabler:chevron-down" color="neutral" variant="solid" class="min-h-10 shadow-sm" aria-label="选择相册 ZIP 下载格式">下载相册</UButton></UDropdownMenu>
    <UButton v-else icon="tabler:download" color="neutral" variant="solid" class="min-h-10 shadow-sm" :disabled="!download.formats[0]?.url" @click="save(download.formats[0]?.url)">{{ download.formats[0]?.url ? '下载相册' : label(download.formats[0]?.status || '') }}</UButton>
  </div>
</template>
