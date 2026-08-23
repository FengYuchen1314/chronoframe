<script setup lang="ts">
const { t } = useI18n()
const {
  availableFilters,
  selectedCounts,
  toggleFilter,
  isFilterSelected,
  clearAllFilters,
  hasActiveFilters,
  activeFilters,
} = usePhotoFilters()
const { shufflePhotos } = usePhotoSort()
const currentTab = ref<'tags' | 'cameras' | 'lenses' | 'cities' | 'ratings'>('tags')
const tabs = computed(() => [
  { key: 'tags' as const, label: t('ui.action.filter.tabs.tags'), icon: 'tabler:tags', count: selectedCounts.value.tags },
  { key: 'cameras' as const, label: t('ui.action.filter.tabs.cameras'), icon: 'tabler:camera', count: selectedCounts.value.cameras },
  { key: 'lenses' as const, label: t('ui.action.filter.tabs.lenses'), icon: 'tabler:aperture', count: selectedCounts.value.lenses },
  { key: 'cities' as const, label: t('ui.action.filter.tabs.cities'), icon: 'tabler:map-pin', count: selectedCounts.value.cities },
  { key: 'ratings' as const, label: t('ui.action.filter.tabs.ratings'), icon: 'tabler:star', count: selectedCounts.value.ratings },
])

const options = computed(() => currentTab.value === 'ratings' ? [] : availableFilters.value[currentTab.value])
</script>

<template>
  <div class="w-[calc(100vw-36px)] max-w-md space-y-3">
    <div class="flex items-center gap-2">
      <UInput v-model="activeFilters.search" icon="tabler:search" :placeholder="t('ui.action.filter.searchPlaceholder')" class="min-w-0 flex-1" />
      <UTooltip :text="t('ui.action.filter.shuffleTooltip')">
        <UButton icon="tabler:dice-3" color="neutral" variant="ghost" @click="shufflePhotos" />
      </UTooltip>
      <UButton v-if="hasActiveFilters" icon="tabler:filter-x" color="neutral" variant="ghost" @click="clearAllFilters" />
    </div>

    <div class="grid grid-cols-5 gap-1 rounded-xl bg-neutral-100/70 p-1 dark:bg-neutral-800/70">
      <button
        v-for="tab in tabs"
        :key="tab.key"
        type="button"
        class="relative flex min-w-0 flex-col items-center gap-0.5 rounded-lg px-1 py-2 text-[10px] font-medium transition"
        :class="currentTab === tab.key ? 'bg-white text-neutral-900 shadow-sm dark:bg-neutral-700 dark:text-white' : 'text-neutral-500 hover:text-neutral-900 dark:hover:text-white'"
        @click="currentTab = tab.key"
      >
        <Icon :name="tab.icon" class="size-4" />
        <span class="w-full truncate">{{ tab.label }}</span>
        <span v-if="tab.count" class="absolute -right-0.5 -top-0.5 rounded-full bg-info-500 px-1 text-[9px] text-white">{{ tab.count }}</span>
      </button>
    </div>

    <div v-if="currentTab === 'ratings'" class="flex flex-col items-center gap-3 py-8">
      <Rating v-model="activeFilters.ratings" size="xl" :allow-half="false" />
      <p class="text-xs font-semibold text-neutral-600 dark:text-neutral-300">
        {{ activeFilters.ratings ? t('ui.action.filter.rating.showStarsAndAbove', activeFilters.ratings) : t('ui.action.filter.rating.showAll') }}
      </p>
    </div>
    <div v-else class="max-h-64 overflow-y-auto pr-1">
      <button
        v-for="option in options"
        :key="option.label"
        type="button"
        class="mb-1 flex w-full items-center justify-between rounded-lg px-2 py-2 text-left text-sm transition hover:bg-neutral-200/70 dark:hover:bg-neutral-800"
        :class="isFilterSelected(currentTab, option.label) && 'bg-info-50 text-info-700 dark:bg-info-950/50 dark:text-info-300'"
        @click="toggleFilter(currentTab, option.label)"
      >
        <span class="truncate">{{ option.label }}</span>
        <span class="ml-3 flex items-center gap-2 text-xs text-neutral-400">
          {{ option.count }}<Icon v-if="isFilterSelected(currentTab, option.label)" name="tabler:check" class="size-4 text-info-500" />
        </span>
      </button>
      <p v-if="!options.length" class="py-10 text-center text-sm text-neutral-500">{{ t(`ui.action.filter.empty.${currentTab}`) }}</p>
    </div>
  </div>
</template>
