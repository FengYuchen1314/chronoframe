import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { PROJECT_LINKS } from '../shared/utils/projectLinks.ts'

test('maintainer, fork and upstream links point to the correct GitHub targets', () => {
  assert.equal(PROJECT_LINKS.profile, 'https://github.com/FengYuchen1314')
  assert.equal(PROJECT_LINKS.repository, `${PROJECT_LINKS.profile}/chronoframe`)
  assert.equal(PROJECT_LINKS.upstream, 'https://github.com/HoshinoSuzumi/chronoframe')
})

test('every supported locale has the two footer labels', () => {
  const directory = new URL('../i18n/locales/', import.meta.url)
  for (const file of readdirSync(directory).filter(file => file.endsWith('.json'))) {
    const { siteFooter } = JSON.parse(readFileSync(new URL(file, directory), 'utf8'))
    assert.ok(siteFooter?.source?.trim(), `${file}: source label`)
    assert.ok(siteFooter?.upstream?.trim(), `${file}: upstream label`)
  }
})

test('README identifies the redevelopment, credits upstream and keeps copyable Compose deployment', () => {
  const readme = readFileSync(new URL('../README.md', import.meta.url), 'utf8')
  for (const url of [PROJECT_LINKS.profile, PROJECT_LINKS.repository, PROJECT_LINKS.upstream]) assert.ok(readme.includes(url))
  assert.match(readme, /^# ChronoFrame（二次开发重构版）/)
  assert.match(readme, /## 原项目介绍/)
  assert.match(readme, /```yaml[\s\S]*image: ghcr\.io\/fengyuchen1314\/chronoframe:latest[\s\S]*- \.\/data:\/app\/data[\s\S]*```/)
  assert.ok(readFileSync(new URL('../LICENSE', import.meta.url), 'utf8').includes('Copyright (c) 2025 Timothy Yin'))
})
