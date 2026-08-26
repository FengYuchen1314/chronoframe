<script setup lang="ts">
import { motion } from 'motion-v'
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
  <motion.div
    :data-photo-id="photo.id"
    class="w-full"
    :initial="props.index < props.firstScreenItems ? { opacity: 0, y: 24, scale: 0.97, filter: 'blur(5px)' } : false"
    :animate="{ opacity: 1, y: 0, scale: 1, filter: 'blur(0px)' }"
    :transition="{ type: 'spring', duration: 0.35, bounce: 0, delay: Math.min(index, 24) * 0.018 }"
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
  </motion.div>
</template>
