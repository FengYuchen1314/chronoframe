<script setup lang="ts">
import type { GalleryPhoto } from '~~/shared/types/photo'

const props = withDefaults(defineProps<{
  photo: GalleryPhoto
  index: number
  firstScreenItems?: number
  selected?: boolean
  selectionMode?: boolean
}>(), { firstScreenItems: 36, selected: false, selectionMode: false })

const emit = defineEmits<{
  activate: [number, MouseEvent | KeyboardEvent]
  contextAction: [GalleryPhoto, number, number]
  select: [number, MouseEvent | KeyboardEvent]
}>()
const forwardActivate = (index: number, event: MouseEvent | KeyboardEvent) => emit('activate', index, event)
const forwardContext = (photo: GalleryPhoto, x: number, y: number) => emit('contextAction', photo, x, y)
const forwardSelect = (index: number, event: MouseEvent | KeyboardEvent) => emit('select', index, event)
</script>

<template>
  <div
    :data-photo-id="photo.id"
    class="w-full"
  >
    <MasonryItemPhoto
      :photo="photo"
      :index="index"
      :selected="selected"
      :selection-mode="selectionMode"
      @activate="forwardActivate"
      @context-action="forwardContext"
      @select="forwardSelect"
    />
  </div>
</template>
