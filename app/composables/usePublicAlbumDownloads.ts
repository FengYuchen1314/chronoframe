import type { PublicAlbumDownload } from '~~/shared/types/downloads'

export function usePublicAlbumDownloads() {
  const downloads = ref<PublicAlbumDownload[]>([])
  let active = false
  let timer: ReturnType<typeof setTimeout> | undefined
  const poll = async () => {
    try { downloads.value = await $fetch<PublicAlbumDownload[]>('/api/album-downloads/public') } catch { /* Gallery browsing remains available if download status is unavailable. */ }
    const pending = downloads.value.some(album => album.formats.some(item => ['queued', 'running'].includes(item.status)))
    if (active) timer = setTimeout(poll, document.hidden ? 60000 : pending ? 5000 : 30000)
  }
  onMounted(() => { active = true; void poll() })
  onBeforeUnmount(() => { active = false; clearTimeout(timer) })
  return { downloads }
}
