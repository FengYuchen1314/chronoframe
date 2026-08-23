<script setup lang="ts">
import { motion } from 'motion-v'
import type { GalleryPhoto } from '~~/shared/types/photo'

defineProps<{ currentPhoto: GalleryPhoto; onClose?: () => void }>()
const isMobile = useMediaQuery('(max-width: 768px)')
const { t } = useI18n()
</script>

<template>
  <motion.aside
    :initial="isMobile ? { opacity: 0, y: 80 } : { opacity: 0, x: 80 }"
    :animate="{ opacity: 1, x: 0, y: 0 }"
    :exit="isMobile ? { opacity: 0, y: 80 } : { opacity: 0, x: 80 }"
    :transition="{ type: 'spring', duration: 0.4, bounce: 0, delay: 0.08 }"
    class="flex flex-col border-white/10 bg-black/25 text-white backdrop-blur-xl dark:bg-black/35"
    :class="isMobile ? 'fixed inset-x-2 bottom-2 z-[120] max-h-[72vh] rounded-xl border' : 'h-full w-80 shrink-0 border-l xl:w-96'"
  >
    <div class="flex shrink-0 items-center justify-between border-b border-white/10 px-4 py-3">
      <h3 class="line-clamp-1 font-black">{{ currentPhoto.title || t('ui.photo.untitled') }}</h3>
      <UButton v-if="isMobile" icon="tabler:x" color="neutral" variant="ghost" size="sm" class="text-white" @click="onClose?.()" />
    </div>

    <div class="min-h-0 flex-1 space-y-6 overflow-y-auto p-4 pb-16 text-sm">
      <p v-if="currentPhoto.description" class="text-justify leading-relaxed text-white/80">{{ currentPhoto.description }}</p>

      <section v-if="currentPhoto.exif?.Rating">
        <h4 class="mb-2 text-xs font-semibold uppercase tracking-wide text-white/55">{{ t('exif.sections.rating') }}</h4>
        <Rating :model-value="currentPhoto.exif.Rating" readonly size="sm" />
      </section>

      <section>
        <h4 class="mb-2 text-xs font-semibold uppercase tracking-wide text-white/55">{{ t('exif.sections.basic') }}</h4>
        <dl class="space-y-2">
          <div class="flex justify-between gap-4"><dt class="text-white/45">{{ t('exif.filename') }}</dt><dd class="min-w-0 truncate text-right">{{ currentPhoto.title || '—' }}</dd></div>
          <div class="flex justify-between gap-4"><dt class="text-white/45">{{ t('exif.dateTaken.title') }}</dt><dd class="text-right">{{ formatGalleryDate(currentPhoto.dateTaken, { dateStyle: 'medium', timeStyle: 'short' }) }}</dd></div>
          <div class="flex justify-between gap-4"><dt class="text-white/45">{{ t('exif.resolution') }}</dt><dd>{{ currentPhoto.width && currentPhoto.height ? `${currentPhoto.width} × ${currentPhoto.height}` : '—' }}</dd></div>
          <div class="flex justify-between gap-4"><dt class="text-white/45">{{ t('exif.fileSize') }}</dt><dd>{{ formatBytes(currentPhoto.fileSize) }}</dd></div>
        </dl>
      </section>

      <section v-if="currentPhoto.exif && Object.values(currentPhoto.exif).some(Boolean)">
        <h4 class="mb-2 text-xs font-semibold uppercase tracking-wide text-white/55">{{ t('exif.sections.deviceInfomation') }}</h4>
        <dl class="space-y-2">
          <div v-if="currentPhoto.exif.Make || currentPhoto.exif.Model" class="flex justify-between gap-4"><dt class="text-white/45">{{ t('exif.camera') }}</dt><dd class="text-right">{{ [currentPhoto.exif.Make, currentPhoto.exif.Model].filter(Boolean).join(' ') }}</dd></div>
          <div v-if="currentPhoto.exif.LensMake || currentPhoto.exif.LensModel" class="flex justify-between gap-4"><dt class="text-white/45">{{ t('exif.lens') }}</dt><dd class="text-right">{{ [currentPhoto.exif.LensMake, currentPhoto.exif.LensModel].filter(Boolean).join(' ') }}</dd></div>
          <div v-if="currentPhoto.exif.FNumber" class="flex justify-between gap-4"><dt class="text-white/45">{{ t('exif.aperture') }}</dt><dd>ƒ/{{ currentPhoto.exif.FNumber }}</dd></div>
          <div v-if="currentPhoto.exif.ExposureTime" class="flex justify-between gap-4"><dt class="text-white/45">{{ t('exif.exposure.time') }}</dt><dd>{{ currentPhoto.exif.ExposureTime }}</dd></div>
          <div v-if="currentPhoto.exif.ISO || currentPhoto.exif.ISOSpeedRatings" class="flex justify-between gap-4"><dt class="text-white/45">{{ t('exif.iso') }}</dt><dd>{{ currentPhoto.exif.ISO || currentPhoto.exif.ISOSpeedRatings }}</dd></div>
          <div v-if="currentPhoto.exif.FocalLength" class="flex justify-between gap-4"><dt class="text-white/45">{{ t('exif.focal.length.actual') }}</dt><dd>{{ currentPhoto.exif.FocalLength }}</dd></div>
        </dl>
      </section>

      <section v-if="currentPhoto.city || currentPhoto.country || currentPhoto.locationName">
        <h4 class="mb-2 text-xs font-semibold uppercase tracking-wide text-white/55">{{ t('exif.gps.title') }}</h4>
        <p>{{ [currentPhoto.locationName, currentPhoto.city, currentPhoto.country].filter(Boolean).join(' · ') }}</p>
      </section>

      <section v-if="currentPhoto.tags.length">
        <h4 class="mb-2 text-xs font-semibold uppercase tracking-wide text-white/55">{{ t('exif.sections.tags') }}</h4>
        <div class="flex flex-wrap gap-1.5"><span v-for="tag in currentPhoto.tags" :key="tag" class="rounded-full bg-white/10 px-2.5 py-1 text-xs text-white/75">{{ tag }}</span></div>
      </section>
    </div>
  </motion.aside>
</template>
