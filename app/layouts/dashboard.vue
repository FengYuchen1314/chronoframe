<script lang="ts" setup>
import type { NavigationMenuItem } from '@nuxt/ui'

const config = useRuntimeConfig()
const toast = useToast()
const {
  authState,
  refreshAuthStatus,
  register,
  login,
  logout,
} = useAdminApi()

const username = ref('')
const password = ref('')
const passwordConfirmation = ref('')
const formError = ref('')
const isSubmitting = ref(false)
const isLoggingOut = ref(false)
const appTitle = computed(() => String(config.public.app.title || 'ChronoFrame'))
const isRegistration = computed(() => !authState.value.initialized)

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

watch(
  () => authState.value.username,
  (value) => {
    if (value && !username.value) username.value = value
  },
  { immediate: true },
)

watch(isRegistration, () => {
  password.value = ''
  passwordConfirmation.value = ''
  formError.value = ''
})

if (import.meta.client) void refreshAuthStatus()

const clearFormError = () => {
  formError.value = ''
}

const submitAuth = async () => {
  if (isSubmitting.value) return

  const normalizedUsername = username.value.trim()
  if (!normalizedUsername) {
    formError.value = '请输入管理员用户名'
    return
  }
  if (Array.from(normalizedUsername).length > 64) {
    formError.value = '管理员用户名不能超过 64 个字符'
    return
  }
  if (Array.from(password.value).length < 12) {
    formError.value = '管理员密码至少需要 12 个字符'
    return
  }
  if (new TextEncoder().encode(password.value).byteLength > 1024) {
    formError.value = '管理员密码不能超过 1024 字节'
    return
  }
  if (isRegistration.value && password.value !== passwordConfirmation.value) {
    formError.value = '两次输入的密码不一致'
    return
  }

  const registering = isRegistration.value
  formError.value = ''
  isSubmitting.value = true

  try {
    if (registering) {
      await register(normalizedUsername, password.value)
    } else {
      await login(normalizedUsername, password.value)
    }

    password.value = ''
    passwordConfirmation.value = ''

    if (authState.value.authenticated) {
      toast.add({
        title: registering ? '管理员账号已创建' : '登录成功',
        color: 'success',
      })
    } else if (registering && authState.value.initialized) {
      formError.value = '管理员账号已创建，请使用新账号登录'
    } else if (!authState.value.error) {
      formError.value = '登录状态未生效，请重试'
    }
  } catch (error) {
    formError.value = getAdminApiErrorMessage(error)
  } finally {
    isSubmitting.value = false
  }
}

const leaveDashboard = async () => {
  if (isLoggingOut.value) return
  isLoggingOut.value = true

  try {
    await logout()
    password.value = ''
    passwordConfirmation.value = ''
    formError.value = ''
    toast.add({ title: '已退出管理后台', color: 'neutral' })
  } catch (error) {
    toast.add({
      title: '退出失败',
      description: getAdminApiErrorMessage(error),
      color: 'error',
    })
  } finally {
    isLoggingOut.value = false
  }
}

useHead({
  titleTemplate: title => `${title ? `${title} | ` : ''}${appTitle.value}`,
})
</script>

<template>
  <div
    v-if="!authState.checked || authState.loading"
    class="flex min-h-svh items-center justify-center px-4 py-10"
  >
    <UCard class="w-full max-w-md">
      <div class="flex flex-col items-center gap-4 py-8 text-center">
        <Icon name="tabler:loader-2" class="size-8 animate-spin text-primary" />
        <div>
          <h1 class="font-semibold">管理后台</h1>
          <p class="mt-1 text-sm text-muted">正在检查管理员状态…</p>
        </div>
      </div>
    </UCard>
  </div>

  <div
    v-else-if="authState.error"
    class="flex min-h-svh items-center justify-center px-4 py-10"
  >
    <UCard class="w-full max-w-md">
      <template #header>
        <div class="flex items-center gap-3">
          <span class="flex size-10 items-center justify-center rounded-lg bg-error/10 text-error">
            <Icon name="tabler:cloud-exclamation" class="size-6" />
          </span>
          <div>
            <h1 class="text-lg font-semibold">无法连接管理服务</h1>
            <p class="text-sm text-muted">管理员状态检查失败</p>
          </div>
        </div>
      </template>

      <UAlert color="error" variant="subtle" :description="authState.error" />
      <UButton block icon="tabler:refresh" class="mt-4" @click="refreshAuthStatus">
        重新检查
      </UButton>
    </UCard>
  </div>

  <div
    v-else-if="!authState.authenticated"
    class="flex min-h-svh items-center justify-center px-4 py-10"
  >
    <UCard class="w-full max-w-md">
      <template #header>
        <div class="flex items-center gap-3">
          <span class="flex size-10 items-center justify-center rounded-lg bg-primary/10 text-primary">
            <Icon :name="isRegistration ? 'tabler:user-plus' : 'tabler:shield-lock'" class="size-6" />
          </span>
          <div>
            <h1 class="text-lg font-semibold">
              {{ isRegistration ? '创建管理员账号' : '登录管理后台' }}
            </h1>
            <p class="text-sm text-muted">
              {{ isRegistration ? '首次使用，请先设置用户名和密码' : '请输入管理员用户名和密码' }}
            </p>
          </div>
        </div>
      </template>

      <form class="space-y-4" @submit.prevent="submitAuth">
        <UAlert
          v-if="formError"
          color="error"
          variant="subtle"
          icon="tabler:alert-circle"
          :description="formError"
        />

        <UFormField label="管理员用户名" help="1–64 个字符" required>
          <UInput
            v-model="username"
            autocomplete="username"
            placeholder="输入用户名"
            icon="tabler:user"
            class="w-full"
            autofocus
            :disabled="isSubmitting"
            @update:model-value="clearFormError"
          />
        </UFormField>

        <UFormField label="管理员密码" help="至少 12 个字符，最多 1024 字节" required>
          <UInput
            v-model="password"
            type="password"
            :autocomplete="isRegistration ? 'new-password' : 'current-password'"
            placeholder="输入密码"
            icon="tabler:lock-password"
            class="w-full"
            :minlength="12"
            :maxlength="1024"
            :disabled="isSubmitting"
            @update:model-value="clearFormError"
          />
        </UFormField>

        <UFormField v-if="isRegistration" label="确认管理员密码" required>
          <UInput
            v-model="passwordConfirmation"
            type="password"
            autocomplete="new-password"
            placeholder="再次输入密码"
            icon="tabler:lock-check"
            class="w-full"
            :minlength="12"
            :maxlength="1024"
            :disabled="isSubmitting"
            @update:model-value="clearFormError"
          />
        </UFormField>

        <UButton
          type="submit"
          block
          :icon="isRegistration ? 'tabler:user-plus' : 'tabler:login-2'"
          :loading="isSubmitting"
        >
          {{ isRegistration ? '创建账号并进入后台' : '登录' }}
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
            <span class="truncate text-xs text-muted">
              {{ authState.username || '管理后台' }}
            </span>
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
          :label="collapsed ? undefined : '退出登录'"
          icon="tabler:logout-2"
          size="lg"
          color="neutral"
          variant="ghost"
          class="w-full"
          :block="collapsed"
          :loading="isLoggingOut"
          @click="leaveDashboard"
        />
      </template>
    </UDashboardSidebar>

    <slot />
  </UDashboardGroup>
</template>
