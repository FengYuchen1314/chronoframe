<script lang="ts" setup>
import type { NavigationMenuItem } from '@nuxt/ui'

const config = useRuntimeConfig()
const {
  adminToken,
  hasAdminToken,
  hydrateAdminToken,
  setAdminToken,
  clearAdminToken,
} = useAdminApi()

const tokenDraft = ref('')
const tokenError = ref('')
const appTitle = computed(() => String(config.public.app.title || 'ChronoFrame'))

const navItems = computed<NavigationMenuItem[][]>(() => [
  [
    {
      label: '概览',
      icon: 'tabler:dashboard',
      to: '/dashboard',
    },
    {
      label: '相簿',
      icon: 'tabler:album',
      to: '/dashboard/albums',
    },
    {
      label: '格式转换',
      icon: 'tabler:arrows-exchange',
      to: '/dashboard/conversions',
    },
    {
      label: '存储设置',
      icon: 'tabler:database-cog',
      to: '/dashboard/settings/storage',
    },
  ],
  [
    {
      label: '返回相簿',
      icon: 'tabler:photo',
      to: '/',
    },
    {
      label: 'GitHub',
      icon: 'tabler:brand-github',
      to: 'https://github.com/HoshinoSuzumi/chronoframe',
      target: '_blank',
    },
  ],
])

onMounted(() => {
  hydrateAdminToken()
  if (!hasAdminToken.value) tokenDraft.value = adminToken.value
})

const enterDashboard = () => {
  if (!tokenDraft.value.trim()) {
    tokenError.value = '请输入管理员令牌'
    return
  }

  tokenError.value = ''
  setAdminToken(tokenDraft.value)
  tokenDraft.value = ''
}

const leaveDashboard = () => {
  clearAdminToken()
  tokenDraft.value = ''
  tokenError.value = ''
}

useHead({
  titleTemplate: title => `${title ? `${title} | ` : ''}${appTitle.value}`,
})
</script>

<template>
  <div
    v-if="!hasAdminToken"
    class="flex min-h-svh items-center justify-center px-4 py-10"
  >
    <UCard class="w-full max-w-md">
      <template #header>
        <div class="flex items-center gap-3">
          <span class="flex size-10 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <Icon name="tabler:shield-lock" class="size-6" />
          </span>
          <div>
            <h1 class="text-lg font-semibold">管理后台</h1>
            <p class="text-sm text-muted">令牌仅保存在当前浏览器会话</p>
          </div>
        </div>
      </template>

      <form class="space-y-4" @submit.prevent="enterDashboard">
        <UAlert
          color="neutral"
          variant="subtle"
          icon="tabler:info-circle"
          title="需要管理员令牌"
          description="每个后台请求都会统一使用 X-Admin-Token 请求头。"
        />

        <UFormField label="管理员令牌" required :error="tokenError">
          <UInput
            v-model="tokenDraft"
            type="password"
            autocomplete="current-password"
            placeholder="输入令牌"
            icon="tabler:key"
            class="w-full"
            autofocus
            @update:model-value="tokenError = ''"
          />
        </UFormField>

        <UButton type="submit" block icon="tabler:login-2">
          进入后台
        </UButton>
      </form>
    </UCard>
  </div>

  <UDashboardGroup v-else>
    <UDashboardSidebar
      id="cframe-dashboard-sidebar"
      resizable
      collapsible
      mode="drawer"
      :min-size="8"
      :max-size="12"
      :ui="{ footer: 'border-t border-default' }"
      :toggle="{
        color: 'primary',
        variant: 'subtle',
        class: 'rounded-full',
      }"
    >
      <template #toggle>
        <UDashboardSidebarToggle variant="soft" />
      </template>

      <template #header="{ collapsed }">
        <div v-if="!collapsed" class="flex items-center gap-2">
          <img src="/favicon.svg" class="h-8 w-auto shrink-0" alt="" />
          <div class="flex min-w-0 flex-col">
            <NuxtLink to="/" class="line-clamp-1 text-lg font-medium">
              {{ appTitle }}
            </NuxtLink>
            <span class="text-xs text-muted">管理后台</span>
          </div>
        </div>
        <img v-else src="/favicon.svg" class="mx-auto size-8" alt="" />
      </template>

      <template #default="{ collapsed }">
        <UNavigationMenu
          :collapsed="collapsed"
          :items="navItems[0]"
          orientation="vertical"
        />
        <UNavigationMenu
          :collapsed="collapsed"
          :items="navItems[1]"
          orientation="vertical"
          class="mt-auto"
        />
      </template>

      <template #footer="{ collapsed }">
        <UButton
          :label="collapsed ? undefined : '清除令牌并退出'"
          icon="tabler:logout-2"
          size="lg"
          color="neutral"
          variant="ghost"
          class="w-full"
          :block="collapsed"
          @click="leaveDashboard"
        />
      </template>
    </UDashboardSidebar>

    <slot />
  </UDashboardGroup>
</template>
