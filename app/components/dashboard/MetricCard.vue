<script lang="ts" setup>
withDefaults(defineProps<{
  label: string
  value: string | number
  icon: string
  tone?: 'primary' | 'info' | 'success' | 'warning' | 'neutral'
  hint?: string
  to?: string
}>(), {
  tone: 'primary',
  hint: '',
  to: undefined,
})

const toneClasses = {
  primary: 'bg-primary/10 text-primary',
  info: 'bg-info/10 text-info',
  success: 'bg-success/10 text-success',
  warning: 'bg-warning/10 text-warning',
  neutral: 'bg-elevated text-muted',
}
</script>

<template>
  <component
    :is="to ? resolveComponent('NuxtLink') : 'div'"
    :to="to"
    class="group flex min-w-0 items-center gap-3 rounded-xl border border-default bg-default p-4 shadow-xs transition"
    :class="to ? 'hover:border-accented hover:bg-elevated/40' : ''"
  >
    <span class="flex size-10 shrink-0 items-center justify-center rounded-lg" :class="toneClasses[tone]">
      <Icon :name="icon" class="size-5" />
    </span>
    <span class="min-w-0 flex-1">
      <span class="block text-xs font-medium text-muted">{{ label }}</span>
      <span class="mt-0.5 block truncate text-lg font-semibold text-highlighted">{{ value }}</span>
      <span v-if="hint" class="mt-0.5 block truncate text-xs text-muted">{{ hint }}</span>
    </span>
    <Icon v-if="to" name="tabler:chevron-right" class="size-4 shrink-0 text-dimmed transition group-hover:translate-x-0.5 group-hover:text-primary" />
  </component>
</template>
