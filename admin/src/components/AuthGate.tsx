import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { Alert, Button, Input, Label, TextField } from '@heroui/react'
import { adminApi, getAdminApiErrorMessage, getAuthSnapshot } from '../lib/api'
import type { AdminAuthState } from '../lib/types'

/**
 * 登录 / 首次注册卡片。对应 Vue 侧 layouts/dashboard.vue 的未登录分支。
 * 校验规则与文案逐条对齐现网（用户名 1–64 字符、密码 ≥12 字符且 ≤1024 字节、两次一致）。
 */
export default function AuthGate({
  auth,
  busy,
  error,
  setError,
  setBusy,
  siteTitle,
  siteAvatar,
}: {
  auth: AdminAuthState
  busy: boolean
  error: string
  setError: (value: string) => void
  setBusy: (value: boolean) => void
  siteTitle: string
  siteAvatar: string
}) {
  // initialized=false 时是「创建唯一的管理员账号」，与注册竞争（409）后会切到登录。
  const registration = !auth.initialized

  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirmation, setConfirmation] = useState('')

  // 注册态切到登录态时清空密码（例如另一个标签页抢先完成了注册）。
  useEffect(() => {
    setPassword('')
    setConfirmation('')
  }, [registration])

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (busy) return
    setError('')

    if (!username.trim() || username.trim().length > 64) {
      setError('用户名需为 1–64 个字符')
      return
    }
    if (password.length < 12 || new TextEncoder().encode(password).byteLength > 1024) {
      setError('密码至少 12 个字符，最多 1024 字节')
      return
    }
    if (registration && password !== confirmation) {
      setError('两次输入的密码不一致')
      return
    }

    setBusy(true)
    try {
      if (registration) await adminApi.register(username, password)
      else await adminApi.login(username, password)
      // 接口 200 不代表登录态真的生效（Cookie 被浏览器策略丢弃、会话立刻失效都可能）。
      // register/login 内部已经刷过一次 status，这里读最新快照再确认一遍。
      // 不能用 `auth` prop：它是本次渲染的旧值，刷新后的结果还没进到这个闭包里。
      const fresh = getAuthSnapshot()
      if (!fresh.authenticated) setError(fresh.error || '登录状态未生效，请重试')
    } catch (cause) {
      setError(getAdminApiErrorMessage(cause))
    } finally {
      // 无论成败都清空密码字段，避免凭据留在表单里。
      setBusy(false)
      setPassword('')
      setConfirmation('')
    }
  }

  const shownError = auth.error || error

  return (
    <div
      style={{
        display: 'grid',
        placeItems: 'center',
        minHeight: '100svh',
        padding: 24,
        background: 'var(--surface-secondary)',
      }}
    >
      <div
        style={{
          width: '100%',
          maxWidth: 420,
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          borderRadius: 12,
          padding: 24,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          <img
            src={siteAvatar}
            alt=""
            width={32}
            height={32}
            style={{ width: 32, height: 32, borderRadius: 6, objectFit: 'cover' }}
          />
          <strong>{siteTitle}</strong>
          <span className="admin-help">管理后台</span>
        </div>

        <h1 style={{ fontSize: 24, fontWeight: 600, margin: '16px 0 8px' }}>
          {registration ? '创建管理员账号' : '管理员登录'}
        </h1>
        <p className="admin-help">
          {registration ? '首次使用，请创建唯一的管理员账号。' : '管理相册、公开下载与网站设置。'}
        </p>

        {shownError ? (
          <div style={{ marginTop: 16 }}>
            <Alert status="danger">
              <Alert.Content>
                <Alert.Description>
                  <span className="preserve-newlines">{shownError}</span>
                </Alert.Description>
              </Alert.Content>
            </Alert>
          </div>
        ) : null}

        <form onSubmit={submit} style={{ marginTop: 20, display: 'flex', flexDirection: 'column', gap: 14 }}>
          <TextField value={username} onChange={setUsername} isDisabled={busy}>
            <Label>用户名</Label>
            <Input autoComplete="username" maxLength={64} />
          </TextField>

          <TextField value={password} onChange={setPassword} isDisabled={busy}>
            <Label>密码</Label>
            <Input
              type="password"
              autoComplete={registration ? 'new-password' : 'current-password'}
              placeholder="至少 12 个字符"
            />
          </TextField>

          {registration ? (
            <TextField value={confirmation} onChange={setConfirmation} isDisabled={busy}>
              <Label>确认密码</Label>
              <Input type="password" autoComplete="new-password" />
            </TextField>
          ) : null}

          <Button type="submit" variant="primary" size="lg" fullWidth isDisabled={busy}>
            {registration ? '创建账号并进入后台' : '登录'}
          </Button>
        </form>

        {auth.error ? (
          <Button
            variant="ghost"
            fullWidth
            className="mt-3"
            onPress={() => void adminApi.refreshAuthStatus()}
          >
            重试连接
          </Button>
        ) : null}

        <div style={{ marginTop: 20, textAlign: 'center' }}>
          <a href="/" style={{ fontSize: 14, color: 'var(--link)' }}>返回相册</a>
        </div>
      </div>
    </div>
  )
}
