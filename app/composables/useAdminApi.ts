type AdminMethod = 'GET' | 'HEAD' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

interface AdminFetchOptions {
  method?: AdminMethod
  body?: unknown
  query?: Record<string, string | number | boolean | null | undefined>
  headers?: HeadersInit
  signal?: AbortSignal
}

interface AuthStatusResponse {
  initialized: boolean
  authenticated: boolean
  username?: string
}

interface AdminAuthState {
  checked: boolean
  loading: boolean
  initialized: boolean
  authenticated: boolean
  username: string
  error: string
}

interface ApiErrorShape {
  data?: {
    error?: string
    message?: string
  }
  message?: string
  status?: number
  statusCode?: number
  response?: {
    status?: number
  }
}

let pendingAuthStatusRequest: Promise<void> | null = null

const readBrowserCookie = (name: string): string => {
  if (!import.meta.client) return ''

  const prefix = `${encodeURIComponent(name)}=`
  const entry = document.cookie
    .split(';')
    .map(cookie => cookie.trim())
    .find(cookie => cookie.startsWith(prefix))

  if (!entry) return ''
  const encodedValue = entry.slice(prefix.length)
  try {
    return decodeURIComponent(encodedValue)
  } catch {
    return encodedValue
  }
}

const responseStatusOf = (error: unknown): number | undefined => {
  if (!error || typeof error !== 'object') return undefined
  const candidate = error as ApiErrorShape
  return candidate.response?.status ?? candidate.statusCode ?? candidate.status
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
  const authState = useState<AdminAuthState>('chronoframe-admin-auth', () => ({
    checked: false,
    loading: false,
    initialized: false,
    authenticated: false,
    username: '',
    error: '',
  }))

  const applyAuthStatus = (status: AuthStatusResponse) => {
    authState.value.checked = true
    authState.value.initialized = Boolean(status.initialized)
    authState.value.authenticated = Boolean(status.authenticated)
    authState.value.username = status.username?.trim() || ''
    authState.value.error = ''
  }

  const markUnauthenticated = () => {
    authState.value.checked = true
    authState.value.authenticated = false
    authState.value.error = ''
  }

  const request = async <T>(
    url: string,
    options: AdminFetchOptions = {},
  ): Promise<T> => {
    const method = options.method || 'GET'
    const headers = new Headers(options.headers)
    headers.set('X-Requested-With', 'ChronoFrame')

    if (method !== 'GET' && method !== 'HEAD') {
      const csrfToken = readBrowserCookie('cf_csrf')
      if (csrfToken) headers.set('X-CSRF-Token', csrfToken)
    }

    try {
      return await $fetch<T>(url, {
        ...options,
        method,
        headers,
        credentials: 'include',
      } as never)
    } catch (error) {
      if (responseStatusOf(error) === 401) markUnauthenticated()
      throw error
    }
  }

  const refreshAuthStatus = async (): Promise<void> => {
    if (pendingAuthStatusRequest) return await pendingAuthStatusRequest

    pendingAuthStatusRequest = (async () => {
      authState.value.loading = true
      authState.value.error = ''

      try {
        const status = await request<AuthStatusResponse>('/api/auth/status')
        applyAuthStatus(status)
      } catch (error) {
        authState.value.checked = true
        authState.value.authenticated = false
        authState.value.error = getAdminApiErrorMessage(error)
      } finally {
        authState.value.loading = false
      }
    })()

    try {
      await pendingAuthStatusRequest
    } finally {
      pendingAuthStatusRequest = null
    }
  }

  const register = async (username: string, password: string) => {
    try {
      await request('/api/auth/register', {
        method: 'POST',
        body: { username: username.trim(), password },
      })
    } catch (error) {
      // Another browser may win the one-time registration race. Refresh immediately so
      // this tab switches from registration to login instead of presenting a stale form.
      if (responseStatusOf(error) === 409) await refreshAuthStatus()
      throw error
    }
    await refreshAuthStatus()
  }

  const login = async (username: string, password: string) => {
    await request('/api/auth/login', {
      method: 'POST',
      body: { username: username.trim(), password },
    })
    await refreshAuthStatus()
  }

  const logout = async () => {
    const sendLogoutRequest = () => request('/api/auth/logout', { method: 'POST' })

    try {
      await sendLogoutRequest()
    } catch (error) {
      const status = responseStatusOf(error)
      if (status === 403) {
        await refreshAuthStatus()
        try {
          await sendLogoutRequest()
        } catch (retryError) {
          if (responseStatusOf(retryError) !== 401) throw retryError
        }
      } else if (status !== 401) {
        throw error
      }
    }

    markUnauthenticated()
  }

  const adminFetch = async <T>(
    url: string,
    options: AdminFetchOptions = {},
  ): Promise<T> => {
    if (!authState.value.checked || authState.value.loading) {
      await refreshAuthStatus()
    }

    if (!authState.value.authenticated) {
      throw new Error('请先登录管理员账号')
    }

    return await request<T>(url, options)
  }

  return {
    authState,
    refreshAuthStatus,
    register,
    login,
    logout,
    adminFetch,
  }
}
