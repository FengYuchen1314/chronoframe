<script setup lang="ts">
import { AnimatePresence, motion } from 'motion-v'

defineProps<{ total: number; dateRangeText: string }>()
const config = useRuntimeConfig()
const colorMode = useColorMode()
const { t } = useI18n()
const { hasActiveFilters, selectedCounts } = usePhotoFilters()
const { currentSortOption, currentSortIcon, availableSorts, setSortOption } = usePhotoSort()
const repoHover = ref(false)

const isDark = computed({
  get: () => colorMode.value === 'dark',
  set: value => { colorMode.preference = value ? 'dark' : 'light' },
})
const selectedFilterCount = computed(() => Object.values(selectedCounts.value).reduce((sum, value) => sum + value, 0))
</script>

<template>
  <header class="relative w-full overflow-hidden">
    <div
      class="absolute inset-0 -z-10 scale-110 bg-cover bg-center opacity-35 blur-3xl"
      :style="{ backgroundImage: `url(${config.public.app.avatarUrl})` }"
    />
    <div class="absolute inset-0 -z-10 bg-white/50 dark:bg-neutral-900/50" />

    <div class="flex flex-col items-center gap-2 pb-0 pt-6">
      <a href="/dashboard" class="group flex flex-col items-center gap-2" :aria-label="t('title.dashboard')">
        <img :src="config.public.app.avatarUrl" class="size-16 rounded-full object-cover shadow-sm transition-transform group-hover:scale-105" :alt="t('ui.photo.avatarAlt')" />
        <h1 class="mb-2 text-2xl font-bold text-neutral-900 dark:text-white/90">{{ config.public.app.title }}</h1>
      </a>

      <div class="space-y-1 text-center text-neutral-600 dark:text-white/35">
        <p class="text-xs font-medium">
          <template v-if="total">{{ t('ui.stats.totalPhotosWithRange', { range: dateRangeText, count: total }) }}</template>
          <template v-else>{{ t('ui.stats.noPhotosTip') }}</template>
        </p>
        <p v-if="config.public.app.slogan" class="font-[Pacifico]">{{ config.public.app.slogan }}</p>
      </div>

      <nav class="flex items-center gap-0 rounded-full bg-white/35 p-1 dark:bg-neutral-900/50" :aria-label="t('title.gallery')">
        <UTooltip :text="t('title.albums')">
          <UButton to="/" icon="tabler:photo" color="neutral" variant="soft" size="sm" class="cursor-pointer rounded-full bg-transparent" />
        </UTooltip>

        <UPopover>
          <UTooltip :text="t('ui.action.filter.tooltip')">
            <UChip inset size="sm" color="info" :show="selectedFilterCount > 0">
              <UButton icon="tabler:filter" :color="hasActiveFilters ? 'info' : 'neutral'" variant="soft" size="sm" class="cursor-pointer rounded-full bg-transparent" />
            </UChip>
          </UTooltip>
          <template #content>
            <UCard variant="glassmorphism" class="border-white/40 bg-white/85 shadow-xl backdrop-blur-2xl dark:border-white/10 dark:bg-neutral-900/85">
              <OverlayFilterPanel />
            </UCard>
          </template>
        </UPopover>

        <UPopover>
          <UTooltip :text="t('ui.action.sort.tooltip')">
            <UButton :icon="currentSortIcon" :color="currentSortOption?.key === 'dateTaken-desc' ? 'neutral' : 'info'" variant="soft" size="sm" class="cursor-pointer rounded-full bg-transparent" />
          </UTooltip>
          <template #content>
            <UCard variant="glassmorphism" class="w-64 border-white/40 bg-white/90 shadow-xl backdrop-blur-2xl dark:border-white/10 dark:bg-neutral-900/90">
              <p class="mb-2 px-2 text-sm font-bold">{{ t('ui.action.sort.title') }}</p>
              <UButton
                v-for="sort in availableSorts"
                :key="sort.key"
                block
                :icon="sort.icon"
                :label="t(sort.labelI18n)"
                :variant="currentSortOption?.key === sort.key ? 'soft' : 'ghost'"
                :color="currentSortOption?.key === sort.key ? 'info' : 'neutral'"
                class="mb-1 justify-start"
                @click="setSortOption(sort.key)"
              />
            </UCard>
          </template>
        </UPopover>

        <UTooltip :text="t('ui.action.theme.tooltip')">
          <UButton :icon="isDark ? 'tabler:sun' : 'tabler:moon'" color="neutral" variant="soft" size="sm" class="cursor-pointer rounded-full bg-transparent" @click="isDark = !isDark" />
        </UTooltip>
        <UTooltip :text="t('ui.action.dashboard.tooltip')">
          <UButton href="/dashboard" external icon="tabler:dashboard" color="info" variant="soft" size="sm" class="cursor-pointer rounded-full bg-transparent" />
        </UTooltip>
      </nav>

      <footer class="mt-1 flex w-full items-center justify-between gap-2 bg-neutral-200/50 px-2 py-1.5 text-xs font-medium text-neutral-500 dark:bg-neutral-900/50">
        <span class="truncate">© {{ new Date().getFullYear() }} {{ config.public.app.author || config.public.app.title }}</span>
        <a
          href="https://github.com/HoshinoSuzumi/chronoframe"
          target="_blank"
          rel="noopener noreferrer"
          class="inline-flex items-center gap-1 hover:underline"
          @mouseenter="repoHover = true"
          @mouseleave="repoHover = false"
        >
          <Icon name="tabler:brand-github" /> ChronoFrame
          <AnimatePresence>
            <motion.span v-if="repoHover" :initial="{ width: 0, opacity: 0 }" :animate="{ width: 'auto', opacity: 1 }" :exit="{ width: 0, opacity: 0 }" class="overflow-hidden whitespace-nowrap">
              ({{ config.public.VERSION }})
            </motion.span>
          </AnimatePresence>
        </a>
      </footer>
    </div>
  </header>
</template>
