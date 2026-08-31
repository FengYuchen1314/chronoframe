// Run on the VPS against the disposable workflow fixture ONLY, never production.
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
const base = process.env.CF_TEST_BASE || 'http://127.0.0.1:18325'
const url = new URL(base)
assert.equal(url.hostname, '127.0.0.1', 'isolated loopback fixture required')
assert.equal(url.port, '18325', 'only the dedicated workflow fixture is allowed')
const cookies = new Map()
async function request(path, { method = 'GET', body, expected = 200 } = {}) {
  const headers = { 'X-Requested-With': 'ChronoFrame', Cookie: [...cookies].map(([key, value]) => `${key}=${value}`).join('; ') }
  if (cookies.has('cf_csrf')) headers['X-CSRF-Token'] = decodeURIComponent(cookies.get('cf_csrf'))
  if (body && !(body instanceof FormData)) { headers['Content-Type'] = 'application/json'; body = JSON.stringify(body) }
  const response = await fetch(`${base}${path}`, { method, headers, body })
  for (const cookie of response.headers.getSetCookie()) { const [entry] = cookie.split(';'); const index = entry.indexOf('='); cookies.set(entry.slice(0, index), entry.slice(index + 1)) }
  assert.equal(response.status, expected, `${method} ${path}: ${response.status} ${response.status !== expected ? await response.text() : ''}`)
  return response
}
const json = async (path, options) => (await request(path, options)).json()
await request('/api/auth/login', { method: 'POST', body: { username: 'ant-test', password: process.env.CF_TEST_PASSWORD || 'Isolated-Ant-Download-Test-2026!' } })
const original = await json('/api/albums')
const created = []
try {
  for (const name of ['workflow-e2e-A', 'workflow-e2e-B']) {
    const album = await json('/api/albums', { method: 'POST', body: { name, description: 'Disposable workflow regression' }, expected: 201 })
    created.push(album.id)
  }
  const bytes = await readFile(new URL('../public/favicon-96x96.png', import.meta.url))
  const uploads = await Promise.allSettled(Array.from({ length: 10 }, async (_, index) => {
    const body = new FormData()
    body.append('files', new Blob([bytes], { type: 'image/png' }), `workflow-${index}.png`)
    await request(`/api/albums/${created[index < 7 ? 0 : 1]}/photos`, { method: 'POST', body })
  }))
  assert.equal(uploads.filter(result => result.status === 'fulfilled').length, 10)
  const first = await json(`/api/albums/${created[0]}`)
  const second = await json(`/api/albums/${created[1]}`)
  assert.equal(first.photoCount, 7); assert.equal(second.photoCount, 3)
  const metadata = { name: 'workflow-e2e-renamed', description: 'Unified metadata save', displayCreatedDate: '2024-01-02', photoDateStart: '2023-01-01', photoDateEnd: '2023-12-31' }
  const edited = await json(`/api/albums/${created[0]}`, { method: 'PATCH', body: metadata })
  for (const [key, value] of Object.entries(metadata)) assert.equal(edited[key], value)
  const automatic = await json(`/api/albums/${created[0]}`, { method: 'PATCH', body: { ...metadata, displayCreatedDate: null, photoDateStart: null, photoDateEnd: null } })
  assert.equal(automatic.photoDateStart, null); assert.equal(automatic.displayCreatedDate, null)
  await request(`/api/albums/${created[0]}`, { method: 'PATCH', body: { photoDateStart: '2026-02-01', photoDateEnd: '2026-01-01' }, expected: 400 })
  await request(`/api/albums/${created[0]}/cover`, { method: 'PUT', body: { photoId: first.photos[0].id } })
  assert.equal((await json(`/api/albums/${created[0]}`)).coverPhotoId, first.photos[0].id)
  const orderedIds = [...created].reverse().concat(original.map(album => album.id))
  await request('/api/albums/order', { method: 'POST', body: { albumIds: orderedIds } })
  assert.deepEqual((await json('/api/albums')).map(album => album.id), orderedIds)
  await request('/api/album-downloads/settings/bulk', { method: 'PUT', body: { target: { scope: 'selected', albumIds: created }, settings: { enabled: true, formats: ['jpeg', 'webp'], maxImageBytes: 1500000, maxZipBytes: 0 } } })
  let downloads
  for (let attempt = 0; attempt < 60; attempt++) {
    downloads = await json('/api/album-downloads')
    const jobs = downloads.jobs.filter(job => created.includes(job.albumId) && job.revision === downloads.settings.find(setting => setting.albumId === job.albumId)?.revision)
    assert.ok(!jobs.some(job => job.status === 'failed'), 'fixture ZIP generation failed')
    if (jobs.length === 4 && jobs.every(job => job.status === 'ready')) break
    await new Promise(resolve => setTimeout(resolve, 500))
  }
  for (const id of created) {
    const config = downloads.settings.find(setting => setting.albumId === id)
    assert.deepEqual(config.formats, ['jpeg', 'webp'])
    const ready = downloads.jobs.filter(job => job.albumId === id && job.revision === config.revision && job.status === 'ready')
    assert.equal(ready.length, 2)
    const archive = await request(`/api/albums/${id}/downloads/jpeg`)
    assert.equal(new Uint8Array(await archive.arrayBuffer())[0], 0x50)
  }
  for (const path of ['/api/storage-migrations', '/api/thumbnails/rebuilds/latest', '/api/s3-cleanups/latest']) await json(path)
  const removed = await json('/api/photos/delete', { method: 'POST', body: { photoIds: [first.photos[0].id, first.photos[1].id] } })
  assert.equal(removed.deleted, 2)
  const remaining = await json(`/api/albums/${created[0]}`)
  assert.equal(remaining.photos.length, 5); assert.equal(remaining.coverSource, 'auto')
  console.log('PASS: concurrent upload destinations, unified metadata, automatic dates, cover, ordering, bulk ZIP settings, downloads, task endpoints, batch deletion')
} finally {
  for (const id of created) await request(`/api/albums/${id}`, { method: 'DELETE' })
  await request('/api/albums/order', { method: 'POST', body: { albumIds: original.map(album => album.id) } })
}
