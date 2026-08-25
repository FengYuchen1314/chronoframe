import pkg from './package.json'
import i18n, { dayjsLocales } from './i18n/i18n.options'

export default defineNuxtConfig({
  compatibilityDate: '2025-07-15',
  ssr: false,
  devtools: { enabled: true },

  modules: [
    '@nuxt/ui',
    '@nuxt/fonts',
    '@nuxt/icon',
    '@vueuse/nuxt',
    'motion-v/nuxt',
    'dayjs-nuxt',
    '@nuxtjs/i18n',
  ],

  css: ['~/assets/css/tailwind.css'],
  components: [{ path: '~/components/ui', pathPrefix: false }, '~/components'],

  runtimeConfig: {
    public: {
      VERSION: pkg.version,
    },
  },

  nitro: {
    preset: 'static',
    prerender: {
      crawlLinks: false,
      routes: ['/'],
    },
  },

  app: {
    head: {
      meta: [
        { name: 'viewport', content: 'width=device-width, initial-scale=1' },
        { name: 'theme-color', content: '#ffffff' },
      ],
      link: [
        { rel: 'icon', href: '/favicon.svg', type: 'image/svg+xml' },
        { rel: 'manifest', href: '/site.webmanifest' },
      ],
    },
  },

  colorMode: {
    storageKey: 'cframe-color-mode',
  },

  icon: {
    clientBundle: { scan: true },
  },

  fonts: {
    families: [
      { name: 'Rubik', weights: [400, 500, 600, 700], global: true },
      { name: 'Noto Sans SC', weights: [400, 500, 600, 700], global: true },
    ],
  },

  dayjs: {
    locales: dayjsLocales,
    plugins: [
      'relativeTime',
      'utc',
      'timezone',
      'duration',
      'localizedFormat',
      'isBetween',
    ],
    defaultTimezone: 'Asia/Shanghai',
  },

  i18n,
})
