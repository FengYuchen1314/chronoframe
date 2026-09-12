import { useEffect, useSyncExternalStore } from 'react'
import { getAuthSnapshot, subscribeAuth } from '../lib/api'
import { useSiteSettings } from '../lib/store'
import type { AdminAuthState } from '../lib/types'

/** 订阅全局鉴权状态。来源是模块级单例，跨页面存活。 */
export const useAuth = (): AdminAuthState =>
  useSyncExternalStore(subscribeAuth, getAuthSnapshot, getAuthSnapshot)

/** 与现网一致的移动端断点：≤991px 视为移动端（不是 768，也不是 1024）。 */
export const useIsMobile = (): boolean => {
  const query = '(max-width: 991px)'
  const subscribe = (listener: () => void) => {
    if (typeof window === 'undefined') return () => {}
    const media = window.matchMedia(query)
    media.addEventListener('change', listener)
    return () => media.removeEventListener('change', listener)
  }
  const getSnapshot = () =>
    typeof window === 'undefined' ? false : window.matchMedia(query).matches
  return useSyncExternalStore(subscribe, getSnapshot, () => false)
}

/**
 * 文档标题。对应 Vue 侧每页的 `useHead({ title })` 叠加 `app.vue:82` 的
 * titleTemplate，最终形态是 `${pageTitle} | ${站点名}`；pageTitle 为空时只留站点名。
 *
 * 注意后台有两页的文档标题与页面 H1 故意不同（spec-settings.md:27）：
 * 存储页标题「存储设置」而 H1 是「存储与维护」，设置页标题「站点设置」而 H1 是「网站设置」。
 */
export const useDocumentTitle = (pageTitle: string): void => {
  const site = useSiteSettings()
  useEffect(() => {
    document.title = pageTitle ? `${pageTitle} | ${site.title}` : site.title
  }, [pageTitle, site.title])
}
