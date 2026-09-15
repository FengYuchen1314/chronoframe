import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { PROJECT_LINKS } from '../shared/utils/projectLinks.ts'

const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8')

test('maintainer, fork and upstream links point to the correct GitHub targets', () => {
  assert.equal(PROJECT_LINKS.profile, 'https://github.com/Uniseem')
  assert.equal(PROJECT_LINKS.repository, `${PROJECT_LINKS.profile}/open-gallery`)
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

test('README only introduces features and credits the original author', () => {
  const readme = read('README.md')
  assert.match(readme, /^# Open Gallery\n/)
  assert.deepEqual(readme.match(/^## .+$/gm), ['## 功能', '## 致谢'])
  assert.ok(readme.includes(PROJECT_LINKS.upstream), 'README credits the upstream project')
  assert.ok(readme.includes('Timothy Yin'), 'README names the original author')
  assert.ok(readme.includes('(DEPLOYMENT.md)'), 'README points to the deployment guide')
  assert.doesNotMatch(readme, /```yaml/, 'deployment steps belong in DEPLOYMENT.md')
})

test('MIT copyright notice of the original author is retained', () => {
  assert.ok(read('LICENSE').includes('Copyright (c) 2025 Timothy Yin'))
})

// 文档里的 Compose 必须与仓库根目录的 docker-compose.yml 逐字一致，
// 否则按文档手动复制的用户会拿到与 curl 下载不同的配置。
test('DEPLOYMENT.md embeds docker-compose.yml verbatim', () => {
  const blocks = [...read('DEPLOYMENT.md').matchAll(/```yaml\n([\s\S]*?)```/g)].map(match => match[1])
  assert.equal(blocks.length, 1, 'exactly one yaml block')
  assert.equal(blocks[0], read('docker-compose.yml'))
  assert.match(blocks[0], /image: ghcr\.io\/uniseem\/open-gallery:latest/)
  assert.match(blocks[0], /- \.\/data:\/app\/data/)
})
