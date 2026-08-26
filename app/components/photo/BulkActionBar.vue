<script setup lang="ts">
import { AnimatePresence, motion } from 'motion-v'
import type { GalleryPhoto } from '~~/shared/types/photo'
import type { PhotoExportFormat } from '~/composables/usePhotoActions'

const props = defineProps<{ selected: GalleryPhoto[]; allCount: number }>()
const emit = defineEmits<{ cancel: []; selectAll: [] }>()
const isMobile = useMediaQuery('(max-width: 767px)')
const format = ref<PhotoExportFormat>('webp')
const { downloadMany, transfer } = usePhotoActions()
const progress = computed(() => transfer.value.total
  ? Math.round(transfer.value.completed / transfer.value.total * 100)
  : 0)

const startDownload = () => downloadMany(props.selected, format.value, isMobile.value)
</script>

<template>
  <Teleport to="body">
    <AnimatePresence>
      <motion.div
        v-if="selected.length"
        class="fixed inset-x-0 bottom-0 z-[90] px-2 pb-[max(.5rem,env(safe-area-inset-bottom))] md:bottom-5 md:px-4"
        :initial="{ opacity: 0, y: 42, scale: 0.98 }"
        :animate="{ opacity: 1, y: 0, scale: 1 }"
        :exit="{ opacity: 0, y: 24, scale: 0.98 }"
        :transition="{ type: 'spring', duration: 0.28, bounce: 0 }"
      >
        <div class="mx-auto max-w-2xl overflow-hidden rounded-2xl border border-white/15 bg-neutral-900/92 text-white shadow-2xl backdrop-blur-2xl">
          <div v-if="transfer.active" class="h-1 bg-white/10"><div class="h-full bg-primary transition-[width] duration-300" :style="{ width: `${progress}%` }" /></div>
          <div class="flex items-center gap-2 p-2 sm:p-2.5">
            <button type="button" class="grid size-10 shrink-0 place-items-center rounded-xl transition hover:bg-white/10 active:scale-95" :disabled="transfer.active" aria-label="退出多选" @click="emit('cancel')"><Icon name="tabler:x" class="size-5" /></button>
            <div class="min-w-0 flex-1 px-1">
              <p class="text-sm font-semibold">{{ transfer.active ? transfer.label : `已选择 ${selected.length} 张` }}</p>
              <p class="truncate text-[11px] text-white/50">{{ transfer.active ? `${transfer.completed} / ${transfer.total}` : (isMobile ? '将按顺序逐张下载' : '将合并为一个 ZIP') }}</p>
            </div>
            <button v-if="selected.length < allCount" type="button" class="hidden rounded-xl px-3 py-2 text-xs text-white/70 transition hover:bg-white/10 sm:block" :disabled="transfer.active" @click="emit('selectAll')">全选</button>
            <label class="relative shrink-0">
              <span class="sr-only">导出格式</span>
              <select v-model="format" class="h-10 appearance-none rounded-xl border border-white/10 bg-white/8 py-0 pl-3 pr-8 text-xs font-semibold text-white outline-none transition focus:border-primary" :disabled="transfer.active">
                <option value="webp" class="text-black">WEBP</option>
                <option value="png" class="text-black">PNG</option>
                <option value="jpg" class="text-black">JPG / JPEG</option>
              </select>
              <Icon name="tabler:chevron-down" class="pointer-events-none absolute right-2 top-1/2 size-4 -translate-y-1/2 text-white/60" />
            </label>
            <button type="button" class="flex h-10 shrink-0 items-center gap-2 rounded-xl bg-primary px-3.5 text-sm font-semibold text-inverted shadow-lg transition hover:brightness-105 active:scale-[.98] disabled:opacity-55" :disabled="transfer.active" :aria-label="isMobile ? '逐张下载所选图片' : '将所选图片下载为 ZIP'" @click="startDownload">
              <Icon :name="transfer.active ? 'tabler:loader-2' : (isMobile ? 'tabler:download' : 'tabler:file-zip')" class="size-4" :class="transfer.active && 'animate-spin'" />
              <span>{{ isMobile ? '逐张下载' : '下载 ZIP' }}</span>
            </button>
          </div>
        </div>
      </motion.div>
    </AnimatePresence>
  </Teleport>
</template>
