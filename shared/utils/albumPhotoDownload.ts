/** Tablets with desktop UA (including iPadOS) still use the touch download flow. */
export function isTouchAlbumDownloadDevice(userAgent: string, touchPoints: number, coarsePointer: boolean, narrow: boolean) {
  return narrow || coarsePointer || /Android|iPhone|iPad|iPod/i.test(userAgent)
    || (/Macintosh|MacIntel/i.test(userAgent) && touchPoints > 1)
}

export async function downloadAlbumSequence<T, B>(items: T[], options: {
  start: number
  signal: AbortSignal
  load: (item: T, signal: AbortSignal) => Promise<B>
  save: (body: B, item: T) => void
  progress: (completed: number) => void
  pause?: (signal: AbortSignal) => Promise<void>
}) {
  for (let index = options.start; index < items.length; index++) {
    options.signal.throwIfAborted()
    const item = items[index]!
    const body = await options.load(item, options.signal)
    // An ignored/late network completion must not trigger a download after Stop.
    options.signal.throwIfAborted()
    options.save(body, item)
    options.progress(index + 1)
    if (index + 1 < items.length) await options.pause?.(options.signal)
  }
}
