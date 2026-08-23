<script setup lang="ts">
const { photos, status } = usePhotos()
useHead({ title: '' })
</script>

<template>
  <div class="relative min-h-svh px-1">
    <ClientOnly>
      <MasonryRoot :photos="photos" :status="status" />
      <template #fallback>
        <div class="fixed inset-0 grid place-items-center">
          <Icon name="tabler:loader-2" class="size-8 animate-spin text-primary" />
        </div>
      </template>
    </ClientOnly>
    <slot />
  </div>
</template>
