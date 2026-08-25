<script lang="ts" setup>
import type { NavigationMenuItem } from '@nuxt/ui'

const toast = useToast()
const { settings: siteSettings, ensureSiteSettings } = useSiteSettings()
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
const appTitle = computed(() => siteSettings.value.title || 'ChronoFrame')
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
  ],
  [
    {
      label: '站点外观',
      icon: 'tabler:palette',
      to: '/dashboard/settings/general',
    },
    {
      label: '存储中心',
      icon: 'tabler:database-cog',
      to: '/dashboard/settings/storage',
    },
  ],
  [
    {
      label: '返回相簿',
      icon: 'tabler:external-link',
      to: '/',
    },
    {
      label: 'GitHub',
      icon: 'tabler:brand-github',
      to: 'https://github.com/FengYuchen1314/chronoframe',
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

if (import.meta.client) {
  void refreshAuthStatus()
  void ensureSiteSettings().catch(() => undefined)
}

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
    class="dashboard-surface flex min-h-svh items-center justify-center px-4 py-10"
  >
    <div class="w-full max-w-sm rounded-3xl border border-default bg-default/90 p-8 text-center shadow-xl backdrop-blur-xl">
      <div class="flex flex-col items-center gap-4 py-4">
        <span class="flex size-14 items-center justify-center rounded-2xl bg-primary/10 text-primary">
          <Icon name="tabler:loader-2" class="size-7 animate-spin" />
        </span>
        <div>
          <h1 class="text-lg font-semibold text-highlighted">正在进入管理后台</h1>
          <p class="mt-1 text-sm text-muted">正在检查管理员状态…</p>
        </div>
      </div>
    </div>
  </div>

  <div
    v-else-if="authState.error"
    class="dashboard-surface flex min-h-svh items-center justify-center px-4 py-10"
  >
    <UCard class="w-full max-w-md shadow-xl">
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
    class="dashboard-surface relative flex min-h-svh items-center justify-center overflow-hidden px-4 py-10"
  >
    <div class="pointer-events-none absolute -left-32 -top-32 size-96 rounded-full bg-primary/10 blur-3xl" />
    <div class="pointer-events-none absolute -bottom-40 -right-28 size-[30rem] rounded-full bg-info/10 blur-3xl" />

    <div class="relative grid w-full max-w-5xl overflow-hidden rounded-3xl border border-default bg-default/90 shadow-2xl backdrop-blur-xl lg:grid-cols-[1.05fr_0.95fr]">
      <section class="hidden min-h-[620px] flex-col justify-between bg-gradient-to-br from-primary via-pink-500 to-purple-600 p-10 text-white lg:flex">
        <div>
          <div class="flex items-center gap-3">
            <img :src="siteSettings.avatarUrl" class="size-11 rounded-2xl bg-white/20 object-cover ring-1 ring-white/20" alt="" />
            <div>
              <p class="text-lg font-semibold">{{ appTitle }}</p>
              <p class="text-sm text-white/70">内容管理工作台</p>
            </div>
          </div>

          <div class="mt-24 max-w-md">
            <p class="text-sm font-semibold uppercase tracking-[0.2em] text-white/70">ChronoFrame Studio</p>
            <h1 class="mt-4 text-4xl font-semibold leading-tight">把相簿、存储和转换任务放在一个清晰的工作流里。</h1>
            <p class="mt-5 leading-7 text-white/75">创建相簿后上传，转换过程实时可见，旧图始终由管理员最终确认。</p>
          </div>
        </div>

        <div class="grid grid-cols-3 gap-3 text-sm">
          <div class="rounded-2xl bg-white/10 p-4 backdrop-blur"><Icon name="tabler:album" class="mb-2 size-5" /><p>相簿优先</p></div>
          <div class="rounded-2xl bg-white/10 p-4 backdrop-blur"><Icon name="tabler:progress" class="mb-2 size-5" /><p>进度可见</p></div>
          <div class="rounded-2xl bg-white/10 p-4 backdrop-blur"><Icon name="tabler:shield-check" class="mb-2 size-5" /><p>安全可控</p></div>
        </div>
      </section>

      <section class="flex min-h-[560px] flex-col justify-center px-6 py-10 sm:px-12 lg:min-h-[620px]">
        <div class="mx-auto w-full max-w-sm">
          <span class="flex size-12 items-center justify-center rounded-2xl bg-primary/10 text-primary lg:hidden">
            <Icon :name="isRegistration ? 'tabler:user-plus' : 'tabler:shield-lock'" class="size-6" />
          </span>
          <p class="mt-6 text-xs font-semibold uppercase tracking-[0.18em] text-primary">{{ appTitle }}</p>
          <h1 class="mt-2 text-3xl font-semibold tracking-tight text-highlighted">
            {{ isRegistration ? '创建管理员账号' : '欢迎回来' }}
          </h1>
          <p class="mt-2 text-sm leading-6 text-muted">
            {{ isRegistration ? '首次打开后台的人可以创建唯一管理员账号，创建后立即进入工作台。' : '登录后管理相簿、转换任务和存储连接。' }}
          </p>

      <form class="mt-8 space-y-5" @submit.prevent="submitAuth">
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
          size="lg"
          :icon="isRegistration ? 'tabler:user-plus' : 'tabler:login-2'"
          :loading="isSubmitting"
        >
          {{ isRegistration ? '创建账号并进入后台' : '登录' }}
        </UButton>
      </form>
          <p class="mt-6 flex items-start gap-2 text-xs leading-5 text-muted">
            <Icon name="tabler:lock" class="mt-0.5 size-4 shrink-0" />
            管理员凭据只发送给当前 ChronoFrame 服务，不会写入浏览器 URL 或站点设置。
          </p>
        </div>
      </section>
    </div>
  </div>

  <UDashboardGroup v-else class="dashboard-surface">
    <UDashboardSidebar
      id="cframe-dashboard-sidebar"
      resizable
      collapsible
      mode="drawer"
      :min-size="15"
      :max-size="20"
      class="border-r border-default bg-default/90 backdrop-blur-xl"
      :ui="{ header: 'border-b border-default', footer: 'border-t border-default' }"
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
        <div v-if="!collapsed" class="flex min-w-0 items-center gap-3 py-1">
          <img :src="siteSettings.avatarUrl" class="size-10 shrink-0 rounded-xl bg-elevated object-cover ring-1 ring-default" alt="" />
          <div class="flex min-w-0 flex-col">
            <NuxtLink to="/dashboard" class="line-clamp-1 font-semibold text-highlighted">
              {{ appTitle }}
            </NuxtLink>
            <span class="truncate text-xs text-muted">内容管理工作台</span>
          </div>
        </div>
        <img v-else :src="siteSettings.avatarUrl" class="mx-auto size-9 rounded-xl object-cover" alt="" />
      </template>

      <template #default="{ collapsed }">
        <p v-if="!collapsed" class="mb-2 px-3 pt-2 text-[11px] font-semibold uppercase tracking-[0.16em] text-dimmed">工作区</p>
        <UNavigationMenu
          :collapsed="collapsed"
          :items="navItems[0]"
          orientation="vertical"
        />
        <p v-if="!collapsed" class="mb-2 mt-6 px-3 text-[11px] font-semibold uppercase tracking-[0.16em] text-dimmed">系统</p>
        <UNavigationMenu
          :collapsed="collapsed"
          :items="navItems[1]"
          orientation="vertical"
        />
        <div class="mt-auto">
          <p v-if="!collapsed" class="mb-2 px-3 text-[11px] font-semibold uppercase tracking-[0.16em] text-dimmed">快捷入口</p>
          <UNavigationMenu
            :collapsed="collapsed"
            :items="navItems[2]"
            orientation="vertical"
          />
        </div>
      </template>

      <template #footer="{ collapsed }">
        <div class="flex min-w-0 items-center gap-2" :class="collapsed ? 'justify-center' : ''">
          <span v-if="!collapsed" class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-elevated text-sm font-semibold text-highlighted">
            {{ (authState.username || 'A').slice(0, 1).toUpperCase() }}
          </span>
          <div v-if="!collapsed" class="min-w-0 flex-1">
            <p class="truncate text-sm font-medium text-highlighted">{{ authState.username || '管理员' }}</p>
            <p class="truncate text-xs text-muted">管理员</p>
          </div>
          <UButton icon="tabler:logout-2" color="neutral" variant="ghost" aria-label="退出登录" :loading="isLoggingOut" @click="leaveDashboard" />
        </div>
      </template>
    </UDashboardSidebar>

    <slot />
  </UDashboardGroup>
</template>
