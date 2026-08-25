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
  <section class="relative overflow-hidden rounded-2xl border border-primary/20 bg-gradient-to-br from-primary/10 via-default to-info/10 px-5 py-6 shadow-sm sm:px-7 sm:py-7">
    <div class="pointer-events-none absolute -right-12 -top-20 size-56 rounded-full bg-primary/10 blur-3xl" />
    <div class="pointer-events-none absolute -bottom-20 left-1/3 size-44 rounded-full bg-info/10 blur-3xl" />

    <div class="relative flex flex-col gap-5 lg:flex-row lg:items-center lg:justify-between">
      <div class="flex min-w-0 items-start gap-4">
        <span class="flex size-12 shrink-0 items-center justify-center rounded-2xl bg-primary text-inverted shadow-lg shadow-primary/20">
          <Icon :name="icon" class="size-6" />
        </span>
        <div class="min-w-0">
          <p class="text-xs font-semibold uppercase tracking-[0.18em] text-primary">{{ eyebrow }}</p>
          <h1 class="mt-1 text-2xl font-semibold tracking-tight text-highlighted sm:text-3xl">{{ title }}</h1>
          <p class="mt-2 max-w-3xl text-sm leading-6 text-muted sm:text-base">{{ description }}</p>
        </div>
      </div>

      <div v-if="$slots.actions" class="flex shrink-0 flex-wrap items-center gap-2 lg:justify-end">
        <slot name="actions" />
      </div>
    </div>

    <div v-if="$slots.default" class="relative mt-6">
      <slot />
    </div>
  </section>
</template>
