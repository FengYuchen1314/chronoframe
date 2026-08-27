<script setup lang="ts">
import type { GalleryPhoto } from '~~/shared/types/photo'
import { masonryLayout } from '~~/shared/utils/masonryLayout'

const props = withDefaults(defineProps<{
  items: Array<{ id: string; photo: GalleryPhoto; index: number }>
  columnWidth?: number
  gap?: number
  minColumns?: number
  maxColumns?: number
  firstColumnOffset?: number
}>(), { columnWidth: 280, gap: 4, minColumns: 2, maxColumns: 8, firstColumnOffset: 0 })
const container = ref<HTMLElement>()
const width = ref(0)
useResizeObserver(container, entries => {
  const nextWidth = entries[0]?.contentRect.width || 0
  if (Math.abs(width.value - nextWidth) > 0.5) width.value = nextWidth
})
const columns = computed(() => masonryLayout(props.items.map(item => item.photo.aspectRatio), width.value,
  props.columnWidth, props.gap, props.minColumns, props.maxColumns, props.firstColumnOffset))
</script>

<template>
  <div ref="container" class="grid w-full items-start" :style="{ gridTemplateColumns: `repeat(${columns.length || minColumns}, minmax(0, 1fr))`, gap: `${gap}px` }">
    <div v-for="(column, index) in columns" :key="index" class="masonry-column flex min-w-0 flex-col" :style="{ gap: `${gap}px`, paddingTop: index === 0 ? `${firstColumnOffset}px` : undefined }">
      <div v-for="itemIndex in column" :key="items[itemIndex]!.id" class="masonry-item min-w-0">
        <slot :item="items[itemIndex]!" />
      </div>
    </div>
  </div>
</template>
