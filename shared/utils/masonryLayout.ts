/** Pack known image aspect ratios without mounting/measuring each DOM item. */
export function masonryLayout(ratios: Array<number | null>, width: number, desiredWidth = 280, gap = 4, minColumns = 2, maxColumns = 8, firstOffset = 0) {
  if (!Number.isFinite(width) || width <= 0) return [] as number[][]
  const count = Math.max(minColumns, Math.min(maxColumns, Math.floor((width + gap) / (desiredWidth + gap))))
  const columnWidth = Math.max(1, (width - gap * (count - 1)) / count)
  const columns = Array.from({ length: count }, () => [] as number[])
  const heights = Array.from({ length: count }, (_, index) => index === 0 ? firstOffset : 0)
  for (let index = 0; index < ratios.length; index++) {
    let shortest = 0
    for (let column = 1; column < count; column++) {
      if (heights[column]! < heights[shortest]!) shortest = column
    }
    const ratio = ratios[index]
    columns[shortest]!.push(index)
    heights[shortest]! += columnWidth / (ratio && Number.isFinite(ratio) && ratio > 0 ? ratio : 1.2) + gap
  }
  return columns
}
