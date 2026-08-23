import type { RouterConfig } from 'nuxt/schema'

export default <RouterConfig>{
  scrollBehavior(to, from, savedPosition) {
    if (savedPosition) return savedPosition

    const isPhotoRoute = (name: unknown) => String(name || '').startsWith('photoId')
    if (isPhotoRoute(to.name) || isPhotoRoute(from.name)) return false

    return { top: 0 }
  },
}
