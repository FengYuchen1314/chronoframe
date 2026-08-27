import type { RouterConfig } from 'nuxt/schema'

export default <RouterConfig>{
  scrollBehavior(to, from, savedPosition) {
    // Opening, switching, and closing a photo must not move its mounted gallery.
    if (to.path === from.path && (to.query.photo || from.query.photo)) return false
    if (savedPosition) return savedPosition

    const isPhotoRoute = (name: unknown) => String(name || '').startsWith('photoId')
    if (isPhotoRoute(to.name) || isPhotoRoute(from.name)) return false

    return { top: 0 }
  },
}
