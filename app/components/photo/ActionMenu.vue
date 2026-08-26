<script setup lang="ts">
import { AnimatePresence, motion } from 'motion-v'
import type { GalleryPhoto } from '~~/shared/types/photo'
import type { PhotoExportFormat } from '~/composables/usePhotoActions'

const props = withDefaults(defineProps<{
  open: boolean
  x?: number
  y?: number
  photo?: GalleryPhoto | null
  allowSelect?: boolean
}>(), { x: 0, y: 0, photo: null, allowSelect: false })

const emit = defineEmits<{ close: []; select: [GalleryPhoto | null] }>()
const isMobile = useMediaQuery('(max-width: 767px)')
const activeGroup = ref<'copy' | 'download' | null>(null)
const { downloadOne, copyOne, transfer } = usePhotoActions()
const formats: Array<{ value: PhotoExportFormat; label: string; detail: string }> = [
  { value: 'webp', label: 'WEBP', detail: '体积最小' },
  { value: 'png', label: 'PNG', detail: '无损兼容' },
  { value: 'jpg', label: 'JPG / JPEG', detail: '通用照片' },
]

const desktopPosition = computed(() => {
  if (!import.meta.client) return { left: '12px', top: '12px' }
  return {
    left: `${Math.max(12, Math.min(props.x, window.innerWidth - 304))}px`,
    top: `${Math.max(12, Math.min(props.y, window.innerHeight - 390))}px`,
  }
})

watch(() => props.open, (open) => {
  if (!open) activeGroup.value = null
})

const chooseFormat = async (format: PhotoExportFormat) => {
  const photo = props.photo
  const group = activeGroup.value
  if (!photo || !group || transfer.value.active) return
  emit('close')
  if (group === 'copy') await copyOne(photo, format)
  else await downloadOne(photo, format)
}
</script>

<template>
  <Teleport to="body">
    <AnimatePresence>
      <div v-if="open" class="fixed inset-0 z-[140]" @contextmenu.prevent>
        <button type="button" class="absolute inset-0 cursor-default bg-black/0" aria-label="关闭图片菜单" @click="emit('close')" />
        <motion.div
          role="menu"
          class="photo-action-menu fixed z-[141] overflow-hidden border border-white/10 bg-neutral-900/92 text-white shadow-2xl backdrop-blur-2xl"
          :class="isMobile ? 'inset-x-2 bottom-2 rounded-[1.35rem] pb-[env(safe-area-inset-bottom)]' : 'w-72 rounded-2xl'"
          :style="isMobile ? undefined : desktopPosition"
          :initial="isMobile ? { opacity: 0, y: 42, scale: 0.98 } : { opacity: 0, y: -5, scale: 0.96 }"
          :animate="{ opacity: 1, y: 0, scale: 1 }"
          :exit="isMobile ? { opacity: 0, y: 28, scale: 0.98 } : { opacity: 0, y: -3, scale: 0.97 }"
          :transition="{ type: 'spring', duration: 0.24, bounce: 0 }"
          @click.stop
          @dblclick.stop
          @pointerdown.stop
          @pointerup.stop
        >
          <div v-if="photo" class="flex items-center gap-3 border-b border-white/10 px-4 py-3.5">
            <img :src="photo.thumbnailUrl" alt="" class="size-11 rounded-xl object-cover ring-1 ring-white/10" />
            <div class="min-w-0">
              <p class="truncate text-sm font-semibold">{{ photo.title || '未命名图片' }}</p>
              <p class="mt-0.5 text-xs text-white/55">导出自高清 WebP 版本</p>
            </div>
          </div>

          <div class="p-2">
            <template v-if="photo">
              <button type="button" role="menuitem" class="menu-row" @click="activeGroup = activeGroup === 'copy' ? null : 'copy'">
                <span class="menu-icon"><Icon name="tabler:copy" class="size-5" /></span>
                <span class="flex-1 text-left font-medium">复制为</span>
                <Icon name="tabler:chevron-right" class="size-4 transition" :class="activeGroup === 'copy' && 'rotate-90'" />
              </button>
              <button type="button" role="menuitem" class="menu-row" @click="activeGroup = activeGroup === 'download' ? null : 'download'">
                <span class="menu-icon"><Icon name="tabler:download" class="size-5" /></span>
                <span class="flex-1 text-left font-medium">下载为</span>
                <Icon name="tabler:chevron-right" class="size-4 transition" :class="activeGroup === 'download' && 'rotate-90'" />
              </button>

              <AnimatePresence>
                <motion.div v-if="activeGroup" class="mx-1 mb-1 mt-1 grid grid-cols-3 gap-1 overflow-hidden rounded-xl bg-white/6 p-1.5" :initial="{ opacity: 0, height: 0 }" :animate="{ opacity: 1, height: 'auto' }" :exit="{ opacity: 0, height: 0 }" :transition="{ duration: 0.16 }">
                  <button v-for="format in formats" :key="format.value" type="button" class="rounded-lg px-2 py-2.5 text-center transition hover:bg-white/12 active:scale-95" :disabled="transfer.active" @click="chooseFormat(format.value)">
                    <span class="block text-xs font-bold">{{ format.label }}</span>
                    <span class="mt-0.5 block text-[10px] text-white/50">{{ format.detail }}</span>
                  </button>
                </motion.div>
              </AnimatePresence>
            </template>

            <button v-if="allowSelect" type="button" role="menuitem" class="menu-row" :class="photo && 'mt-1 border-t border-white/10 pt-3'" @click="emit('select', photo); emit('close')">
              <span class="menu-icon"><Icon name="tabler:checks" class="size-5" /></span>
              <span class="flex-1 text-left font-medium">多选图片</span>
              <span class="text-xs text-white/45">批量下载</span>
            </button>
          </div>

          <button v-if="isMobile" type="button" class="mx-2 mb-2 block w-[calc(100%_-_1rem)] rounded-xl bg-white/8 py-3 text-sm font-medium active:bg-white/15" @click="emit('close')">取消</button>
        </motion.div>
      </div>
    </AnimatePresence>
  </Teleport>
</template>

<style scoped>
.menu-row { display: flex; min-height: 3rem; width: 100%; align-items: center; gap: 0.75rem; border-radius: 0.75rem; padding: 0.5rem 0.65rem; color: rgb(255 255 255 / 88%); transition: background-color 150ms ease, transform 150ms ease; }
.menu-row:hover { background: rgb(255 255 255 / 9%); }
.menu-row:active { transform: scale(.985); background: rgb(255 255 255 / 14%); }
.menu-icon { display: grid; width: 2rem; height: 2rem; place-items: center; border-radius: .65rem; background: rgb(255 255 255 / 8%); }
</style>
