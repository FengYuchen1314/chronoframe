// API 客户端与全局鉴权状态。
// 逐条对应 Vue 侧的 app/composables/useAdminApi.ts，行为必须一致：
//   - 必带 X-Requested-With: ChronoFrame
//   - 非 GET/HEAD 且存在 cookie `cf_csrf` 时附带 X-CSRF-Token
//   - credentials: 'include'
//   - 任意 401 -> 静默登出（不产生错误文案，只把界面踢回登录卡）
//   - 未登录时 adminFetch 直接抛错，不发请求
import type { AdminAuthState, AuthStatusResponse } from './types'

/* ------------------------------- 错误类型 ------------------------------- */

interface ApiErrorShape {
  data?: { error?: string, message?: string }
  message?: string
  status?: number
  statusCode?: number
  response?: { status?: number }
}

export class ApiError extends Error {
  status?: number
  data?: { error?: string, message?: string }

  constructor(message: string, status?: number, data?: { error?: string, message?: string }) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.data = data
  }
}

const responseStatusOf = (error: unknown): number | undefined => {
  if (!error || typeof error !== 'object') return undefined
  const candidate = error as ApiErrorShape
  return candidate.status ?? candidate.statusCode ?? candidate.response?.status
}

/** 错误文案优先级与现网完全一致，否则提示语会和线上不同。 */
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

/* ------------------------------ 鉴权状态机 ------------------------------ */

const AUTH_INITIAL: AdminAuthState = {
  checked: false,
  loading: false,
  initialized: false,
  authenticated: false,
  username: '',
  error: '',
}

let authState: AdminAuthState = { ...AUTH_INITIAL }
const authListeners = new Set<() => void>()

const setAuthState = (patch: Partial<AdminAuthState>) => {
  authState = { ...authState, ...patch }
  for (const listener of authListeners) listener()
}

export const subscribeAuth = (listener: () => void): (() => void) => {
  authListeners.add(listener)
  return () => { authListeners.delete(listener) }
}

export const getAuthSnapshot = (): AdminAuthState => authState

const applyAuthStatus = (status: AuthStatusResponse) => {
  setAuthState({
    checked: true,
    initialized: Boolean(status.initialized),
    authenticated: Boolean(status.authenticated),
    username: status.username?.trim() || '',
    error: '',
  })
}

const markUnauthenticated = () => {
  setAuthState({ checked: true, authenticated: false, error: '' })
}

/* -------------------------------- 请求层 -------------------------------- */

export type AdminMethod = 'GET' | 'HEAD' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'

export interface AdminFetchOptions {
  method?: AdminMethod
  body?: unknown
  query?: Record<string, string | number | boolean | null | undefined>
  headers?: HeadersInit
  signal?: AbortSignal
}

const readBrowserCookie = (name: string): string => {
  if (typeof document === 'undefined') return ''
  const prefix = `${encodeURIComponent(name)}=`
  const entry = document.cookie
    .split(';')
    .map((cookie) => cookie.trim())
    .find((cookie) => cookie.startsWith(prefix))
  if (!entry) return ''
  const encodedValue = entry.slice(prefix.length)
  try {
    return decodeURIComponent(encodedValue)
  } catch {
    return encodedValue
  }
}

const buildQuery = (query: AdminFetchOptions['query']): string => {
  if (!query) return ''
  const params = new URLSearchParams()
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null) continue
    params.set(key, String(value))
  }
  const serialized = params.toString()
  return serialized ? `?${serialized}` : ''
}

const request = async <T>(url: string, options: AdminFetchOptions = {}): Promise<T> => {
  const method = options.method ?? 'GET'
  const headers = new Headers(options.headers)
  headers.set('X-Requested-With', 'ChronoFrame')

  if (method !== 'GET' && method !== 'HEAD') {
    const csrfToken = readBrowserCookie('cf_csrf')
    if (csrfToken) headers.set('X-CSRF-Token', csrfToken)
  }

  let body: BodyInit | undefined
  if (options.body instanceof FormData) {
    // 交给浏览器自动带 boundary，绝不能手工设置 Content-Type。
    body = options.body
  } else if (options.body !== undefined) {
    headers.set('Content-Type', 'application/json')
    body = JSON.stringify(options.body)
  }

  let response: Response
  try {
    response = await fetch(`${url}${buildQuery(options.query)}`, {
      method,
      headers,
      body,
      credentials: 'include',
      signal: options.signal,
    })
  } catch (cause) {
    if (cause instanceof DOMException && cause.name === 'AbortError') throw cause
    // 网络层失败（离线、DNS、连接中断）没有 HTTP 状态码，保持与 $fetch 相同的兜底文案。
    throw new ApiError('请求失败，请稍后重试')
  }

  const text = await response.text()
  let payload: unknown
  if (text) {
    try {
      payload = JSON.parse(text)
    } catch {
      payload = undefined
    }
  }

  if (!response.ok) {
    if (response.status === 401) markUnauthenticated()
    const data = payload && typeof payload === 'object'
      ? payload as { error?: string, message?: string }
      : undefined
    throw new ApiError(
      data?.error || data?.message || `请求失败（${response.status}）`,
      response.status,
      data,
    )
  }

  return payload as T
}

/* ------------------------------ 对外 API ------------------------------- */

let pendingAuthStatusRequest: Promise<void> | null = null

const refreshAuthStatus = async (): Promise<void> => {
  if (pendingAuthStatusRequest) return await pendingAuthStatusRequest

  pendingAuthStatusRequest = (async () => {
    setAuthState({ loading: true, error: '' })
    try {
      const status = await request<AuthStatusResponse>('/api/auth/status')
      applyAuthStatus(status)
    } catch (error) {
      setAuthState({
        checked: true,
        authenticated: false,
        error: getAdminApiErrorMessage(error),
      })
    } finally {
      setAuthState({ loading: false })
    }
  })()

  try {
    await pendingAuthStatusRequest
  } finally {
    pendingAuthStatusRequest = null
  }
}

const register = async (username: string, password: string): Promise<void> => {
  try {
    await request('/api/auth/register', {
      method: 'POST',
      body: { username: username.trim(), password },
    })
  } catch (error) {
    // 另一个浏览器可能赢下一次性注册竞争。立即刷新状态，让本标签页从注册切到登录，
    // 而不是把一个已经失效的注册表单继续摆在用户面前。
    if (responseStatusOf(error) === 409) await refreshAuthStatus()
    throw error
  }
  await refreshAuthStatus()
}

const login = async (username: string, password: string): Promise<void> => {
  await request('/api/auth/login', {
    method: 'POST',
    body: { username: username.trim(), password },
  })
  await refreshAuthStatus()
}

const logout = async (): Promise<void> => {
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

/**
 * 业务请求入口：先确保登录态已就绪，未登录则直接抛错不发请求。
 */
const adminFetch = async <T>(url: string, options: AdminFetchOptions = {}): Promise<T> => {
  if (!authState.checked || authState.loading) {
    await refreshAuthStatus()
  }
  if (!authState.authenticated) {
    throw new Error('请先登录管理员账号')
  }
  return await request<T>(url, options)
}

export const adminApi = {
  request,
  adminFetch,
  refreshAuthStatus,
  register,
  login,
  logout,
}
