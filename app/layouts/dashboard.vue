<script setup lang="ts">
import { ConfigProvider as AConfigProvider, App as AApp, Layout as ALayout, LayoutSider as ALayoutSider, LayoutHeader as ALayoutHeader, LayoutContent as ALayoutContent, Menu as AMenu, Drawer as ADrawer, Button as AButton, Space as ASpace, Breadcrumb as ABreadcrumb, BreadcrumbItem as ABreadcrumbItem, Card as ACard, Alert as AAlert, Form as AForm, FormItem as AFormItem, Input as AInput, InputPassword as AInputPassword, Spin as ASpin, Avatar as AAvatar, theme } from 'ant-design-vue'
import zhCN from 'ant-design-vue/es/locale/zh_CN'
import '~/assets/css/admin.css'

const route = useRoute()
const router = useRouter()
const { settings, ensureSiteSettings } = useSiteSettings()
const { authState, refreshAuthStatus, register, login, logout } = useAdminApi()
const mobile = useMediaQuery('(max-width: 991px)')
const menuOpen = ref(false)
const busy = ref(false)
const uploads = useAdminUploads()
const error = ref('')
const form = reactive({ username: '', password: '', confirmation: '' })
const registration = computed(() => !authState.value.initialized)
const items = [
  { key: '/dashboard', label: '概览', icon: () => h(resolveComponent('Icon'), { name: 'tabler:dashboard' }) },
  { key: '/dashboard/albums', label: '相册管理', icon: () => h(resolveComponent('Icon'), { name: 'tabler:album' }) },
  { key: '/dashboard/downloads', label: '下载管理', icon: () => h(resolveComponent('Icon'), { name: 'tabler:download' }) },
  { key: '/dashboard/tasks', label: '任务中心', icon: () => h(resolveComponent('Icon'), { name: 'tabler:activity' }) },
  { key: '/dashboard/settings/storage', label: '存储与维护', icon: () => h(resolveComponent('Icon'), { name: 'tabler:database' }) },
  { key: '/dashboard/settings/general', label: '网站设置', icon: () => h(resolveComponent('Icon'), { name: 'tabler:settings' }) },
]
const title = computed(() => items.find(item => item.key === route.path)?.label || '管理后台')
const navigate = ({ key }: { key: string | number }) => { menuOpen.value = false; void router.push(String(key)) }
const submit = async () => {
  if (busy.value) return
  error.value = ''
  if (!form.username.trim() || form.username.trim().length > 64) { error.value = '用户名需为 1–64 个字符'; return }
  if (form.password.length < 12 || new TextEncoder().encode(form.password).byteLength > 1024) { error.value = '密码至少 12 个字符，最多 1024 字节'; return }
  if (registration.value && form.password !== form.confirmation) { error.value = '两次输入的密码不一致'; return }
  busy.value = true
  try {
    if (registration.value) await register(form.username, form.password)
    else await login(form.username, form.password)
    if (!authState.value.authenticated) error.value = authState.value.error || '登录状态未生效，请重试'
  } catch (cause) { error.value = getAdminApiErrorMessage(cause) }
  finally { busy.value = false; form.password = ''; form.confirmation = '' }
}
const signOut = async () => {
  if (uploads.pending.value || uploads.failed.value) { uploads.open.value = true; error.value = '请先处理上传队列，再退出登录。'; return }
  busy.value = true
  try { await logout() } catch (cause) { error.value = getAdminApiErrorMessage(cause) }
  finally { busy.value = false }
}
onMounted(() => { void refreshAuthStatus(); void ensureSiteSettings() })
watch(registration, () => { form.password = ''; form.confirmation = '' })
</script>

<template>
  <AConfigProvider :locale="zhCN" :theme="{ algorithm: theme.defaultAlgorithm, token: { colorPrimary: '#1677ff', borderRadius: 6, fontFamily: '-apple-system,BlinkMacSystemFont,Segoe UI,Noto Sans SC,sans-serif' } }">
    <AApp class="admin-app">
      <div v-if="!authState.checked || authState.loading" class="admin-auth"><ASpin size="large" tip="正在检查登录状态" /></div>
      <div v-else-if="!authState.authenticated" class="admin-auth">
        <ACard class="admin-auth-card" :bordered="false">
          <div class="flex items-center gap-3"><AAvatar shape="square" :src="settings.avatarUrl" /><strong>{{ settings.title }}</strong><span class="admin-help">管理后台</span></div>
          <h1>{{ registration ? '创建管理员账号' : '管理员登录' }}</h1>
          <p class="admin-help">{{ registration ? '首次使用，请创建唯一的管理员账号。' : '管理相册、公开下载与网站设置。' }}</p>
          <AAlert v-if="authState.error || error" type="error" show-icon :message="authState.error || error" />
          <AForm layout="vertical" :model="form" @finish="submit">
            <AFormItem label="用户名" name="username" required><AInput v-model:value="form.username" size="large" autocomplete="username" :maxlength="64" /></AFormItem>
            <AFormItem label="密码" name="password" required><AInputPassword v-model:value="form.password" size="large" :autocomplete="registration ? 'new-password' : 'current-password'" placeholder="至少 12 个字符" /></AFormItem>
            <AFormItem v-if="registration" label="确认密码" name="confirmation" required><AInputPassword v-model:value="form.confirmation" size="large" autocomplete="new-password" /></AFormItem>
            <AButton html-type="submit" type="primary" size="large" block :loading="busy">{{ registration ? '创建账号并进入后台' : '登录' }}</AButton>
          </AForm>
          <AButton v-if="authState.error" block class="mt-3" @click="refreshAuthStatus">重试连接</AButton>
          <div class="mt-5 text-center"><NuxtLink to="/">返回相册</NuxtLink></div>
        </ACard>
      </div>
      <ALayout v-else>
        <ALayoutSider v-if="!mobile" :width="224" theme="light" class="admin-sider">
          <NuxtLink to="/dashboard" class="admin-brand"><img :src="settings.avatarUrl" alt="" /><span>{{ settings.title }}</span></NuxtLink>
          <AMenu mode="inline" :items="items" :selected-keys="[route.path]" @click="navigate" />
        </ALayoutSider>
        <ADrawer v-model:open="menuOpen" placement="left" :width="248" title="管理后台" :body-style="{ padding: 0 }"><AMenu mode="inline" :items="items" :selected-keys="[route.path]" @click="navigate" /></ADrawer>
        <ALayout style="min-width:0">
          <ALayoutHeader class="admin-topbar">
            <ASpace><AButton v-if="mobile" type="text" aria-label="打开导航" @click="menuOpen = true"><Icon name="tabler:menu-2" /></AButton><ABreadcrumb><ABreadcrumbItem>管理后台</ABreadcrumbItem><ABreadcrumbItem>{{ title }}</ABreadcrumbItem></ABreadcrumb></ASpace>
            <ASpace :size="12"><DashboardUploadQueue /><NuxtLink v-if="!mobile" to="/" target="_blank">查看网站</NuxtLink><span v-if="!mobile" class="admin-help">{{ authState.username }}</span><AButton :loading="busy" @click="signOut">退出</AButton></ASpace>
          </ALayoutHeader>
          <ALayoutContent class="admin-content"><AAlert v-if="error" type="error" :message="error" closable @close="error = ''" /><slot /></ALayoutContent>
        </ALayout>
      </ALayout>
    </AApp>
  </AConfigProvider>
</template>
