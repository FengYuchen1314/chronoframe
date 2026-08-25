import type { SiteSettings } from '~/types/dashboard'

export const DEFAULT_SITE_SETTINGS: SiteSettings = {
  title: 'ChronoFrame',
  slogan: 'Frame the moments that matter.',
  author: 'ChronoFrame',
  avatarUrl: '/web-app-manifest-192x192.png',
  theme: 'system',
}

let pendingSiteSettingsRequest: Promise<SiteSettings> | null = null

export function useSiteSettings() {
  const settings = useState<SiteSettings>('chronoframe-site-settings', () => ({
    ...DEFAULT_SITE_SETTINGS,
  }))
  const loaded = useState<boolean>('chronoframe-site-settings-loaded', () => false)
  const loading = useState<boolean>('chronoframe-site-settings-loading', () => false)
  const error = useState<string>('chronoframe-site-settings-error', () => '')

  const applySiteSettings = (value: SiteSettings) => {
    settings.value = {
      title: value.title?.trim() || DEFAULT_SITE_SETTINGS.title,
      slogan: value.slogan?.trim() || '',
      author: value.author?.trim() || '',
      avatarUrl: value.avatarUrl?.trim() || DEFAULT_SITE_SETTINGS.avatarUrl,
      theme: ['light', 'dark', 'system'].includes(value.theme) ? value.theme : 'system',
    } as SiteSettings
    loaded.value = true
    error.value = ''
    return settings.value
  }

  const refreshSiteSettings = async (): Promise<SiteSettings> => {
    if (pendingSiteSettingsRequest) return await pendingSiteSettingsRequest
    loading.value = true
    error.value = ''
    pendingSiteSettingsRequest = $fetch<SiteSettings>('/api/settings/site', {
      credentials: 'include',
    })
      .then(applySiteSettings)
      .catch((requestError) => {
        error.value = getAdminApiErrorMessage(requestError)
        throw requestError
      })
      .finally(() => {
        loading.value = false
        pendingSiteSettingsRequest = null
      })
    return await pendingSiteSettingsRequest
  }

  const ensureSiteSettings = async (): Promise<SiteSettings> => {
    if (loaded.value) return settings.value
    return await refreshSiteSettings()
  }

  return {
    settings,
    loaded,
    loading,
    error,
    applySiteSettings,
    refreshSiteSettings,
    ensureSiteSettings,
  }
}
