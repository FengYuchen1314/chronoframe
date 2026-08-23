const ADMIN_TOKEN_STORAGE_KEY = 'chronoframe:admin-token'

type AdminMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

interface AdminFetchOptions {
  method?: AdminMethod
  body?: unknown
  query?: Record<string, string | number | boolean | null | undefined>
  headers?: HeadersInit
  signal?: AbortSignal
}

interface ApiErrorShape {
  data?: {
    error?: string
    message?: string
  }
  message?: string
  status?: number
  statusCode?: number
}

export function getAdminApiErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (!error || typeof error !== 'object') return '请求失败，请稍后重试'

  const candidate = error as ApiErrorShape
  return (
    candidate.data?.error
    || candidate.data?.message
    || candidate.message
    || '请求失败，请稍后重试'
  )
}

export function useAdminApi() {
  const adminToken = useState<string>('chronoframe-admin-token', () => '')
  const tokenHydrated = useState<boolean>(
    'chronoframe-admin-token-hydrated',
    () => false,
  )

  const hydrateAdminToken = () => {
    if (!import.meta.client || tokenHydrated.value) return

    try {
      adminToken.value = sessionStorage.getItem(ADMIN_TOKEN_STORAGE_KEY) || ''
    } catch {
      adminToken.value = ''
    }
    tokenHydrated.value = true
  }

  hydrateAdminToken()

  const setAdminToken = (value: string) => {
    const nextToken = value.trim()
    adminToken.value = nextToken
    tokenHydrated.value = true

    if (!import.meta.client) return
    try {
      if (nextToken) {
        sessionStorage.setItem(ADMIN_TOKEN_STORAGE_KEY, nextToken)
      } else {
        sessionStorage.removeItem(ADMIN_TOKEN_STORAGE_KEY)
      }
    } catch {
      // Restricted browser storage must not prevent this tab from using the token.
    }
  }

  const clearAdminToken = () => setAdminToken('')
  const hasAdminToken = computed(() => Boolean(adminToken.value))

  const adminFetch = async <T>(
    request: string,
    options: AdminFetchOptions = {},
  ): Promise<T> => {
    if (!adminToken.value) {
      throw new Error('请先输入管理员令牌')
    }

    const headers = new Headers(options.headers)
    headers.set('X-Admin-Token', adminToken.value)

    return await $fetch<T>(request, {
      ...options,
      headers,
    } as never)
  }

  return {
    adminToken,
    hasAdminToken,
    tokenHydrated,
    hydrateAdminToken,
    setAdminToken,
    clearAdminToken,
    adminFetch,
  }
}
