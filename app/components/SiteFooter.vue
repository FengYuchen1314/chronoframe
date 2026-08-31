<script setup lang="ts">
import { PROJECT_LINKS } from '~~/shared/utils/projectLinks'

const { t } = useI18n()
</script>

<template>
  <footer data-site-footer class="site-footer border-t border-neutral-200/60 px-4 text-xs text-neutral-500 dark:border-neutral-800/60 dark:text-neutral-400">
    <div class="mx-auto flex max-w-5xl flex-wrap items-center justify-center gap-x-5 gap-y-1">
      <a :href="PROJECT_LINKS.profile" target="_blank" rel="noopener noreferrer" class="footer-link">
        <Icon name="tabler:brand-github" class="size-4 shrink-0" aria-hidden="true" />
        <span>GitHub · {{ PROJECT_LINKS.maintainer }}</span>
      </a>
      <a :href="PROJECT_LINKS.repository" target="_blank" rel="noopener noreferrer" class="footer-link">{{ t('siteFooter.source') }}</a>
      <a :href="PROJECT_LINKS.upstream" target="_blank" rel="noopener noreferrer" class="footer-link">
        <span>{{ t('siteFooter.upstream') }} · ChronoFrame</span>
      </a>
    </div>
  </footer>
</template>

<style scoped>
.site-footer { padding-top: 1rem; padding-bottom: max(1rem, env(safe-area-inset-bottom)); }
.footer-link { display: inline-flex; min-height: 44px; align-items: center; gap: 6px; border-radius: 6px; transition: color 150ms ease; }
.footer-link:hover { color: var(--ui-primary); }
.footer-link:focus-visible { outline: 2px solid var(--ui-primary); outline-offset: 4px; }
</style>
