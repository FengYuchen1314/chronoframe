<script setup lang="ts">
import { AnimatePresence, motion } from 'motion-v'
import { PROJECT_LINKS } from '~~/shared/utils/projectLinks'

defineProps<{ total: number; dateRangeText: string }>()
const config = useRuntimeConfig()
const { settings: siteSettings } = useSiteSettings()
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
      :style="{ backgroundImage: `url(${siteSettings.avatarUrl})` }"
    />
    <div class="absolute inset-0 -z-10 bg-white/50 dark:bg-neutral-900/50" />

    <div class="flex flex-col items-center gap-2 pb-0 pt-4 sm:pt-6">
      <NuxtLink to="/" class="group flex flex-col items-center gap-2" :aria-label="t('title.albums')">
        <img :src="siteSettings.avatarUrl" class="size-14 rounded-full object-cover shadow-sm transition-transform group-hover:scale-105 sm:size-16" :alt="t('ui.photo.avatarAlt')" />
        <h1 class="mb-2 text-2xl font-bold text-neutral-900 dark:text-white/90">{{ siteSettings.title }}</h1>
      </NuxtLink>

      <div class="space-y-1 text-center text-neutral-600 dark:text-white/35">
        <p class="text-xs font-medium">
          <template v-if="total">{{ t('ui.stats.totalPhotosWithRange', { range: dateRangeText, count: total }) }}</template>
          <template v-else>{{ t('ui.stats.noPhotosTip') }}</template>
        </p>
        <p v-if="siteSettings.slogan" class="font-[Pacifico]">{{ siteSettings.slogan }}</p>
      </div>

      <nav class="flex max-w-full items-center gap-0 rounded-full bg-white/35 p-1 dark:bg-neutral-900/50" :aria-label="t('title.gallery')">
        <UTooltip :text="t('title.albums')">
          <UButton to="/" icon="tabler:photo" color="neutral" variant="soft" size="sm" class="min-h-11 min-w-11 cursor-pointer rounded-full bg-transparent sm:min-h-8 sm:min-w-8" />
        </UTooltip>

        <UPopover>
          <UTooltip :text="t('ui.action.filter.tooltip')">
            <UChip inset size="sm" color="info" :show="selectedFilterCount > 0">
              <UButton icon="tabler:filter" :color="hasActiveFilters ? 'info' : 'neutral'" variant="soft" size="sm" class="min-h-11 min-w-11 cursor-pointer rounded-full bg-transparent sm:min-h-8 sm:min-w-8" />
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
            <UButton :icon="currentSortIcon" :color="currentSortOption?.key === 'dateTaken-desc' ? 'neutral' : 'info'" variant="soft" size="sm" class="min-h-11 min-w-11 cursor-pointer rounded-full bg-transparent sm:min-h-8 sm:min-w-8" />
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
          <UButton :icon="isDark ? 'tabler:sun' : 'tabler:moon'" color="neutral" variant="soft" size="sm" class="min-h-11 min-w-11 cursor-pointer rounded-full bg-transparent sm:min-h-8 sm:min-w-8" @click="isDark = !isDark" />
        </UTooltip>
        <UTooltip :text="t('ui.action.dashboard.tooltip')">
          <UButton href="/dashboard" external icon="tabler:dashboard" color="info" variant="soft" size="sm" class="min-h-11 min-w-11 cursor-pointer rounded-full bg-transparent sm:min-h-8 sm:min-w-8" />
        </UTooltip>
      </nav>

      <footer class="mt-1 flex w-full items-center justify-between gap-2 bg-neutral-200/50 px-3 py-2 text-xs font-medium text-neutral-500 sm:px-2 sm:py-1.5 dark:bg-neutral-900/50">
        <span class="truncate">© {{ new Date().getFullYear() }} {{ siteSettings.author || siteSettings.title }}</span>
        <a
          :href="PROJECT_LINKS.repository"
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
