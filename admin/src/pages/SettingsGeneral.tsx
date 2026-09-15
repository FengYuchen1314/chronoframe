import { useCallback, useEffect, useRef, useState } from 'react'
import { Alert, Button, Card, Input, Label, Radio, RadioGroup, TextField } from '@heroui/react'
import { adminApi, getAdminApiErrorMessage } from '../lib/api'
import { applySiteSettings, normalizeSiteSettings, useSiteSettings } from '../lib/store'
import { setThemePreference } from '../lib/theme'
import { notice } from '../lib/notice'
import { useBeforeUnload, useLeaveConfirm } from '../lib/navigation'
import { Icon } from '../lib/icons'
import type { SiteSettings, SiteTheme } from '../lib/types'
import PageHeader from '../components/PageHeader'
import { useDocumentTitle } from '../lib/hooks'

const DEFAULTS: SiteSettings = {
  title: 'Open Gallery',
  slogan: 'Frame the moments that matter.',
  author: 'Open Gallery',
  avatarUrl: '/web-app-manifest-192x192.png',
  theme: 'system',
}

const THEMES: Array<{ value: SiteTheme, label: string, icon: string }> = [
  { value: 'system', label: '跟随系统', icon: 'device-desktop' },
  { value: 'light', label: '浅色', icon: 'sun' },
  { value: 'dark', label: '深色', icon: 'moon' },
]

export default function SettingsGeneral() {
  useDocumentTitle('站点设置')
  const live = useSiteSettings()

  const [form, setForm] = useState<SiteSettings>(DEFAULTS)
  const [baseline, setBaseline] = useState('')
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [loadError, setLoadError] = useState('')

  const signature = JSON.stringify(form)
  const dirty = Boolean(baseline) && signature !== baseline

  const applyForm = useCallback((value: SiteSettings) => {
    const next: SiteSettings = {
      title: value.title,
      slogan: value.slogan,
      author: value.author,
      avatarUrl: value.avatarUrl,
      theme: value.theme,
    }
    setForm(next)
    setBaseline(JSON.stringify(next))
  }, [])

  // 与概览页同理：重入保护用 ref，依赖数组保持为空。
  // 把 loading 放进依赖会让「请求完成 → setLoading(false) → 回调换身份 → effect 重跑」
  // 变成无限请求循环。
  const inFlight = useRef(false)

  const loadSettings = useCallback(async () => {
    if (inFlight.current) return
    inFlight.current = true
    setLoading(true)
    setLoadError('')
    try {
      const value = await adminApi.adminFetch<SiteSettings>('/api/settings/site')
      applyForm(value)
      // 同步全局站点状态，后台侧栏品牌名与头像随之更新。
      applySiteSettings(value)
    } catch (cause) {
      setLoadError(getAdminApiErrorMessage(cause))
    } finally {
      setLoading(false)
      inFlight.current = false
    }
  }, [applyForm])

  useEffect(() => { void loadSettings() }, [loadSettings])

  useBeforeUnload(dirty)
  useLeaveConfirm(dirty, '站点设置尚未保存，确定要放弃修改吗？')

  const save = async () => {
    if (saving || !dirty) return

    const title = form.title.trim()
    if (!title) {
      notice.add({ title: '网站名称不能为空', color: 'warning' })
      return
    }
    // 按 Unicode 码点计数，与后端一致。
    if (Array.from(title).length > 100
      || Array.from(form.slogan.trim()).length > 200
      || Array.from(form.author.trim()).length > 100) {
      notice.add({ title: '站点文字超出长度限制', color: 'warning' })
      return
    }

    setSaving(true)
    try {
      const payload: SiteSettings = {
        title,
        slogan: form.slogan.trim(),
        author: form.author.trim(),
        avatarUrl: form.avatarUrl.trim(),
        theme: form.theme,
      }
      const saved = await adminApi.adminFetch<SiteSettings>('/api/settings/site', {
        method: 'PUT',
        body: payload,
      })
      applyForm(saved)
      applySiteSettings(saved)
      // 保存成功才切换本机主题（「重新读取」不切换），与现网一致。
      setThemePreference(normalizeSiteSettings(saved).theme)
      notice.add({
        title: '站点设置已保存',
        description: '公开页面已立即使用新的名称、标语、作者、头像和默认主题。',
        color: 'success',
      })
    } catch (cause) {
      notice.add({
        title: '保存站点设置失败',
        description: getAdminApiErrorMessage(cause),
        color: 'error',
      })
    } finally {
      setSaving(false)
    }
  }

  const previewAvatar = form.avatarUrl.trim() || DEFAULTS.avatarUrl

  return (
    <div>
      <PageHeader
        title="网站设置"
        description="配置公开页面的网站名称、标语、作者、头像和默认主题。"
        actions={
          <Button variant="secondary" isDisabled={loading || saving} onPress={() => void loadSettings()}>
            <Icon name="refresh" size={16} />
            重新读取
          </Button>
        }
      />

      {loadError ? (
        <div style={{ marginBottom: 20 }}>
          <Alert status="danger">
            <Alert.Indicator />
            <Alert.Content>
              <Alert.Description>
                <span className="preserve-newlines">{loadError}</span>
              </Alert.Description>
            </Alert.Content>
          </Alert>
        </div>
      ) : null}

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_320px]">
        <Card>
          <Card.Header>
            <Card.Title>基本设置</Card.Title>
          </Card.Header>
          <Card.Content>
            <form
              onSubmit={(event) => { event.preventDefault(); void save() }}
              style={{ display: 'flex', flexDirection: 'column', gap: 18 }}
            >
              <TextField
                value={form.title}
                isDisabled={loading || saving}
                onChange={(title: string) => setForm((previous) => ({ ...previous, title }))}
              >
                <Label>网站名称</Label>
                <Input maxLength={100} />
              </TextField>

              <TextField
                value={form.slogan}
                isDisabled={loading || saving}
                onChange={(slogan: string) => setForm((previous) => ({ ...previous, slogan }))}
              >
                <Label>网站标语</Label>
                <Input maxLength={200} />
              </TextField>
              <p className="admin-help" style={{ marginTop: -10 }}>留空可隐藏</p>

              <TextField
                value={form.author}
                isDisabled={loading || saving}
                onChange={(author: string) => setForm((previous) => ({ ...previous, author }))}
              >
                <Label>作者名称</Label>
                <Input maxLength={100} />
              </TextField>

              <TextField
                value={form.avatarUrl}
                isDisabled={loading || saving}
                onChange={(avatarUrl: string) => setForm((previous) => ({ ...previous, avatarUrl }))}
              >
                <Label>头像 URL</Label>
                <Input maxLength={2048} />
              </TextField>
              <p className="admin-help" style={{ marginTop: -10 }}>
                支持站内路径或完整 HTTP(S) 地址
              </p>

              <div>
                <div style={{ fontSize: 13, color: 'var(--muted)', marginBottom: 8 }}>默认主题</div>
                <RadioGroup
                  value={form.theme}
                  isDisabled={loading || saving}
                  onChange={(value) => setForm((previous) => ({ ...previous, theme: value as SiteTheme }))}
                >
                  {THEMES.map((theme) => (
                    <Radio key={theme.value} value={theme.value}>
                      {theme.label}
                    </Radio>
                  ))}
                </RadioGroup>
              </div>

              <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
                <Button type="submit" variant="primary" isDisabled={!dirty || loading || saving}>
                  保存设置
                </Button>
                <Button
                  variant="ghost"
                  isDisabled={!dirty || loading || saving}
                  onPress={() => void loadSettings()}
                >
                  重置
                </Button>
                {dirty ? <span className="admin-unsaved">有未保存的修改</span> : null}
              </div>
            </form>
          </Card.Content>
        </Card>

        <Card>
          <Card.Header>
            <Card.Title>站点预览</Card.Title>
          </Card.Header>
          <Card.Content>
            <img
              src={previewAvatar}
              alt=""
              width={64}
              height={64}
              style={{ width: 64, height: 64, borderRadius: 10, objectFit: 'cover' }}
            />
            <h2
              style={{
                marginTop: 20,
                fontSize: 20,
                fontWeight: 600,
                overflowWrap: 'anywhere',
              }}
            >
              {form.title || '网站名称'}
            </h2>
            <p className="admin-help" style={{ marginTop: 12, overflowWrap: 'anywhere' }}>
              {form.slogan || '这里显示网站标语'}
            </p>
            <p className="admin-help" style={{ marginTop: 24 }}>
              © {form.author || '作者'}
            </p>
            <Button
              variant="secondary"
              className="mt-4"
              onPress={() => window.open('/', '_blank', 'noopener')}
            >
              查看公开页面
              <Icon name="external-link" size={16} />
            </Button>
            {live.title !== form.title ? (
              <p className="admin-help" style={{ marginTop: 16 }}>
                公开页面当前显示「{live.title}」，保存后才会更新。
              </p>
            ) : null}
          </Card.Content>
        </Card>
      </div>
    </div>
  )
}
