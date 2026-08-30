// Retain at most two Blob URLs instead of accumulating an entire album in memory.
const pendingUrls: string[] = []
export function saveAlbumPhoto(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob)
  pendingUrls.push(url)
  while (pendingUrls.length > 2) URL.revokeObjectURL(pendingUrls.shift()!)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = name
  anchor.style.display = 'none'
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
  // Give the browser time to consume the last dispatched download, even if the
  // dialog is closed immediately. The timer never starts another download.
  window.setTimeout(() => {
    const index = pendingUrls.indexOf(url)
    if (index !== -1) { pendingUrls.splice(index, 1); URL.revokeObjectURL(url) }
  }, 30_000)
}
