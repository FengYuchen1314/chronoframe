<script setup lang="ts">
const props = withDefaults(defineProps<{
  modelValue: number
  allowHalf?: boolean
  allowClear?: boolean
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl'
  readonly?: boolean
}>(), { allowHalf: true, allowClear: true, size: 'md', readonly: false })

const emit = defineEmits<{ 'update:modelValue': [number] }>()
const sizeClass = computed(() => ({ xs: 'size-4', sm: 'size-5', md: 'size-6', lg: 'size-7', xl: 'size-8' })[props.size])
const select = (value: number) => {
  if (!props.readonly) emit('update:modelValue', props.allowClear && props.modelValue === value ? 0 : value)
}
</script>

<template>
  <div class="flex items-center gap-1" role="radiogroup" aria-label="Rating">
    <button
      v-for="value in 5"
      :key="value"
      type="button"
      class="relative transition-transform hover:scale-110 disabled:pointer-events-none"
      :disabled="readonly"
      @click="select(value)"
    >
      <Icon :name="modelValue >= value ? 'tabler:star-filled' : 'tabler:star'" :class="[sizeClass, modelValue >= value ? 'text-yellow-400' : 'text-neutral-400']" />
    </button>
  </div>
</template>
