export interface ImageRect { left: number; top: number; width: number; height: number }

export const VIEWER_RETURN_DURATION = 240

export function returnTransform(source: ImageRect, target: ImageRect): string | null {
  if (![source.left, source.top, source.width, source.height, target.left, target.top, target.width, target.height].every(Number.isFinite)
    || source.width <= 0 || source.height <= 0 || target.width <= 0 || target.height <= 0) return null
  return `translate3d(${target.left - source.left}px, ${target.top - source.top}px, 0) scale(${target.width / source.width}, ${target.height / source.height})`
}

export function viewerNeighborIndices(index: number, count: number): number[] {
  return [index + 1, index - 1, index + 2, index - 2].filter(value => value >= 0 && value < count)
}

export function thumbnailWindow(scrollLeft: number, width: number, count: number, stride: number) {
  const start = Math.max(0, Math.floor(scrollLeft / stride) - 3)
  const end = Math.min(count, Math.ceil((scrollLeft + width) / stride) + 3)
  return { start, end }
}
