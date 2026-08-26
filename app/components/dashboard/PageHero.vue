<script lang="ts" setup>
withDefaults(defineProps<{
  eyebrow?: string
  title: string
  description: string
  icon?: string
}>(), {
  eyebrow: '管理后台',
  icon: 'tabler:sparkles',
})
</script>

<template>
  <section class="flex flex-col gap-4 border-b border-default pb-5 lg:flex-row lg:flex-wrap lg:items-end lg:justify-between">
    <div class="flex min-w-0 items-start gap-3">
      <span class="flex size-10 shrink-0 items-center justify-center rounded-lg border border-default bg-default text-primary shadow-xs">
        <Icon :name="icon" class="size-5" />
      </span>
      <div class="min-w-0">
        <p class="text-xs font-medium text-muted">{{ eyebrow }}</p>
        <h1 class="mt-0.5 text-2xl font-semibold tracking-tight text-highlighted">{{ title }}</h1>
        <p class="mt-1.5 max-w-3xl text-sm leading-6 text-muted">{{ description }}</p>
      </div>
    </div>

    <div v-if="$slots.actions" class="flex shrink-0 flex-wrap items-center gap-2 lg:justify-end">
      <slot name="actions" />
    </div>

    <div v-if="$slots.default" class="w-full lg:basis-full">
      <slot />
    </div>
  </section>
</template>
