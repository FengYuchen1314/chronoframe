import test from 'node:test'
import assert from 'node:assert/strict'
import { albumCoverStack } from '../shared/utils/albumCover.ts'

const photos = [1, 2, 3, 4].map(id => ({ id: String(id), thumbnailUrl: `/photo/${id}` }))
test('manual cover goes first without reordering or duplicating photos', () => {
  const before = structuredClone(photos)
  assert.deepEqual(albumCoverStack({ coverUrl: '/photo/3', photos }).map(p => p.thumbnailUrl), ['/photo/3', '/photo/1', '/photo/2'])
  assert.deepEqual(photos, before)
})
test('uploaded cover works for empty albums and before photos finish loading', () => {
  assert.deepEqual(albumCoverStack({ coverUrl: '/cover/v1', photos: [] }), [{ id: 'cover', thumbnailUrl: '/cover/v1' }])
  assert.equal(albumCoverStack({ coverUrl: '/cover/v1', photos }).length, 3)
})
test('automatic and empty covers preserve existing presentation', () => {
  assert.deepEqual(albumCoverStack({ coverUrl: null, photos }), photos.slice(0, 3))
  assert.deepEqual(albumCoverStack({ coverUrl: null, photos: [] }), [])
  assert.deepEqual(albumCoverStack({ coverUrl: '/photo/1', photos }).map(p => p.thumbnailUrl), ['/photo/1', '/photo/2', '/photo/3'])
})
