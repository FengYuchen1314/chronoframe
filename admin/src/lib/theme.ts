// 后台明暗主题。
//
// 现网后台写死浅色（AConfigProvider 固定 defaultAlgorithm），只有「网站设置」
// 保存时会把 colorMode.preference 同步到本机。这里保持一致：站点设置的默认主题
// 决定后台外观，并额外支持跟随系统——HeroUI 原生就是靠 .dark 类切换的。
import type { SiteTheme } from './types'

let mediaBound = false

const prefersDark = () =>
  typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches

const paint = (theme: SiteTheme) => {
  if (typeof document === 'undefined') return
  const effective = theme === 'system' ? (prefersDark() ? 'dark' : 'light') : theme
  document.documentElement.classList.toggle('dark', effective === 'dark')
  document.documentElement.style.colorScheme = effective
}

/** 记录偏好并着色。标记写在 dataset 上，供系统外观变化的监听判断当前偏好。 */
export function setThemePreference(theme: SiteTheme): void {
  if (typeof document !== 'undefined') document.documentElement.dataset.cfTheme = theme
  paint(theme)

  if (mediaBound || typeof window === 'undefined') return
  mediaBound = true
  // 「跟随系统」时需要在系统外观变化后重新着色，这个监听只装一次。
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    if (document.documentElement.dataset.cfTheme === 'system') paint('system')
  })
}
