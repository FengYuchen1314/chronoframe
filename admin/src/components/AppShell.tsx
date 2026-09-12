import { Suspense, useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { Outlet, useLocation, useNavigate } from 'react-router-dom'
import { Button, Drawer, Spinner } from '@heroui/react'
import { adminApi } from '../lib/api'
import { ensureSiteSettings, uploadSnapshot, useSiteSettings, useUploads } from '../lib/store'
import { useAuth, useIsMobile } from '../lib/hooks'
import { Icon } from '../lib/icons'
import AuthGate from './AuthGate'
import UploadQueueDrawer from './UploadQueueDrawer'

interface NavItem {
  path: string
  label: string
  icon: string
}

// 单一扁平菜单，顺序、文案、图标与现网一致（无分组、无嵌套）。
const NAV: NavItem[] = [
  { path: '/', label: '概览', icon: 'dashboard' },
  { path: '/albums', label: '相册管理', icon: 'album' },
  { path: '/downloads', label: '下载管理', icon: 'download' },
  { path: '/tasks', label: '任务中心', icon: 'activity' },
  { path: '/settings/storage', label: '存储与维护', icon: 'database' },
  { path: '/settings/general', label: '网站设置', icon: 'settings' },
]

export default function AppShell() {
  const auth = useAuth()
  const mobile = useIsMobile()
  const location = useLocation()
  const navigate = useNavigate()
  const settings = useSiteSettings()
  const uploads = useUploads()

  const [menuOpen, setMenuOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    void adminApi.refreshAuthStatus()
    // 站点设置是公开端点，失败不应打断后台；现网此处没有 catch，这里补上避免未处理的 rejection。
    void ensureSiteSettings().catch(() => {})
  }, [])

  // 未登录时整个页面树都不渲染 —— 这同时是页面级数据请求与轮询的隐式门禁。
  if (!auth.checked || auth.loading) {
    return (
      <div style={{ display: 'grid', placeItems: 'center', minHeight: '100svh' }}>
        <Spinner size="lg" />
      </div>
    )
  }

  if (!auth.authenticated) {
    return (
      <AuthGate
        auth={auth}
        busy={busy}
        error={error}
        setError={setError}
        setBusy={setBusy}
        siteTitle={settings.title}
        siteAvatar={settings.avatarUrl}
      />
    )
  }

  const current = NAV.find((item) => item.path === location.pathname)
  const breadcrumb = current?.label ?? '管理后台'

  const go = (path: string) => {
    setMenuOpen(false)
    navigate(path)
  }

  const signOut = async () => {
    // 与现网一致：队列里还有未结束或未确认的上传时，先让用户处理，不直接登出。
    const snapshot = uploadSnapshot()
    if (snapshot.pending || snapshot.failed) {
      uploads.setOpen(true)
      setError('请先处理上传队列，再退出登录。')
      return
    }
    setBusy(true)
    try {
      await adminApi.logout()
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }

  const menu = (
    <nav style={{ display: 'flex', flexDirection: 'column', gap: 2, padding: 8 }}>
      {NAV.map((item) => {
        // 激活态只看 pathname，忽略 query —— 与现网一致（?album= 不会让菜单失去高亮）。
        const active = location.pathname === item.path
        return (
          <Button
            key={item.path}
            variant={active ? 'secondary' : 'ghost'}
            fullWidth
            className="justify-start"
            onPress={() => go(item.path)}
          >
            <Icon name={item.icon} size={18} />
            <span>{item.label}</span>
          </Button>
        )
      })}
    </nav>
  )

  const brand = (
    <button
      type="button"
      onClick={() => go('/')}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        width: '100%',
        height: 64,
        padding: '0 20px',
        background: 'transparent',
        border: 0,
        cursor: 'pointer',
        color: 'inherit',
        overflow: 'hidden',
        whiteSpace: 'nowrap',
        fontSize: 17,
        fontWeight: 600,
      }}
    >
      <img
        src={settings.avatarUrl}
        alt=""
        width={28}
        height={28}
        style={{ width: 28, height: 28, borderRadius: 6, objectFit: 'cover', flexShrink: 0 }}
      />
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{settings.title}</span>
    </button>
  )

  return (
    <div style={{ display: 'flex', minHeight: '100svh', background: 'var(--surface-secondary)' }}>
      {/* 移动端直接不渲染侧栏（现网是 v-if 移除，不是折叠成 mini）。 */}
      {!mobile ? (
        <aside
          style={{
            width: 224,
            flexShrink: 0,
            position: 'sticky',
            top: 0,
            height: '100svh',
            borderRight: '1px solid var(--border)',
            background: 'var(--surface)',
            display: 'flex',
            flexDirection: 'column',
          }}
        >
          {brand}
          <div style={{ overflowY: 'auto' }}>{menu}</div>
        </aside>
      ) : null}

      {mobile ? (
        <Drawer.Root isOpen={menuOpen} onOpenChange={(next: boolean) => { if (!next) setMenuOpen(false) }}>
          <Drawer.Backdrop>
            <Drawer.Content placement="left" className="w-[248px] max-w-[80vw]">
              <Drawer.Dialog>
                <Drawer.Header>
                  <Drawer.Heading>管理后台</Drawer.Heading>
                </Drawer.Header>
                <Drawer.Body style={{ padding: 0 }}>
                  {brand}
                  {menu}
                </Drawer.Body>
              </Drawer.Dialog>
            </Drawer.Content>
          </Drawer.Backdrop>
        </Drawer.Root>
      ) : null}

      <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
        <header
          style={{
            height: 64,
            flexShrink: 0,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 16,
            padding: mobile ? '0 16px' : '0 24px',
            background: 'var(--surface)',
            borderBottom: '1px solid var(--border)',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 12, minWidth: 0 }}>
            {mobile ? (
              <Button
                variant="ghost"
                isIconOnly
                aria-label="打开导航"
                onPress={() => setMenuOpen(true)}
              >
                <Icon name="menu-2" size={20} />
              </Button>
            ) : null}
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
              <span className="admin-help">管理后台</span>
              <Icon name="chevron-right" size={14} style={{ color: 'var(--muted)' }} />
              <span
                style={{
                  fontSize: 14,
                  fontWeight: 500,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
              >
                {breadcrumb}
              </span>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 }}>
            <UploadButton onOpen={() => uploads.setOpen(true)} />
            {!mobile ? (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  onPress={() => window.open('/', '_blank', 'noopener')}
                >
                  查看网站
                  <Icon name="external-link" size={16} />
                </Button>
                {auth.username ? <span className="admin-help">{auth.username}</span> : null}
              </>
            ) : null}
            <Button variant="ghost" size="sm" isDisabled={busy} onPress={() => void signOut()}>
              退出
            </Button>
          </div>
        </header>

        <main style={{ flex: 1, minWidth: 0, padding: mobile ? 16 : 24 }}>
          <div style={{ maxWidth: 1680, margin: '0 auto' }}>
            {error ? (
              <div
                style={{
                  marginBottom: 20,
                  padding: '12px 14px',
                  borderRadius: 8,
                  border: '1px solid var(--danger)',
                  background: 'color-mix(in oklab, var(--danger) 8%, var(--surface))',
                  display: 'flex',
                  alignItems: 'flex-start',
                  justifyContent: 'space-between',
                  gap: 12,
                }}
              >
                <span className="preserve-newlines" style={{ fontSize: 14, color: 'var(--danger)' }}>
                  {error}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  isIconOnly
                  aria-label="关闭"
                  onPress={() => setError('')}
                >
                  <Icon name="x" size={16} />
                </Button>
              </div>
            ) : null}
            <Suspense fallback={<PageFallback />}>
              <Outlet context={{ setShellError: setError }} />
            </Suspense>
          </div>
        </main>
      </div>

      <UploadQueueDrawer />
    </div>
  )
}

/** 懒加载页面时的占位：保持与页面一致的最小高度，避免布局跳动。 */
function PageFallback() {
  return (
    <div
      style={{
        display: 'grid',
        placeItems: 'center',
        gap: 10,
        minHeight: 320,
        color: 'var(--muted)',
      }}
    >
      <Spinner size="md" />
      <span style={{ fontSize: 14 }}>正在加载…</span>
    </div>
  )
}

/** 顶栏「上传队列」入口。存在未确认成功的项时显示红点。 */
function UploadButton({ onOpen }: { onOpen: () => void }) {
  const uploads = useUploads()
  return (
    <Button variant="ghost" size="sm" onPress={onOpen}>
      <span style={{ position: 'relative', display: 'inline-flex' }}>
        <Icon name="cloud-upload" size={18} />
        {uploads.failed > 0 ? (
          <span
            style={{
              position: 'absolute',
              top: -2,
              right: -2,
              width: 8,
              height: 8,
              borderRadius: 999,
              background: 'var(--danger)',
            }}
          />
        ) : null}
      </span>
      <span>上传队列{uploads.pending > 0 ? `（${uploads.pending}）` : ''}</span>
    </Button>
  )
}

/** 供页面向上抛出整体错误（例如相册页的「请先处理上传队列」）。 */
export interface ShellContext {
  setShellError: (message: string) => void
}

export const navItems = NAV
export type { NavItem }
export type { ReactNode }
