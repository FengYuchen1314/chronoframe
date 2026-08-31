import test from 'node:test'
import assert from 'node:assert/strict'
import { createUploadQueue, createUploadQueueState } from '../shared/utils/admin-upload-queue.ts'
import { albumDraftOf, toggleVisibleSelection, validateAlbumDraft } from '../shared/utils/admin-albums.ts'

const tick = () => new Promise(resolve => setImmediate(resolve))
const files = count => Array.from({ length: count }, (_, index) => ({ name: `${index}.png`, size: 123 }))
const album = { id: 'album-a', name: 'A' }
function fixture() {
  const state = createUploadQueueState()
  const calls = []
  const queue = createUploadQueue(state, (file, albumId) => new Promise((resolve, reject) => calls.push({ file, albumId, resolve, reject })), String)
  return { state, calls, queue }
}

test('seven global upload slots, each completion immediately starts the next item', async () => {
  const { state, calls, queue } = fixture()
  queue.enqueue(files(10), album)
  queue.enqueue(files(5), { id: 'album-b', name: 'B' })
  await tick()
  assert.equal(calls.length, 7)
  assert.equal(state.items.filter(item => item.status === 'uploading').length, 7)
  calls[0].resolve()
  await tick()
  assert.equal(calls.length, 8)
  assert.equal(state.items[0].file, undefined)
  assert.equal(state.albumVersions['album-a'], 1)
  queue.pause()
  calls.slice(1).forEach(call => call.resolve())
  await tick()
  assert.equal(state.items.filter(item => item.status === 'uploading').length, 0)
  assert.equal(calls.length, 8)
  queue.resume()
  await tick()
  assert.equal(calls.length, 15)
  assert.deepEqual(calls.slice(10).map(call => call.albumId), Array(5).fill('album-b'))
  calls.slice(8).forEach(call => call.resolve())
  await tick()
  assert.equal(state.items.filter(item => item.status === 'done').length, 15)
  assert.deepEqual(state.albumVersions, { 'album-a': 10, 'album-b': 5 })
})

test('pause never aborts committed work; retries are explicit and respect pause', async () => {
  const { state, calls, queue } = fixture()
  queue.enqueue(files(8), album)
  await tick()
  queue.pause()
  calls[0].reject(new Error('response lost'))
  calls.slice(1).forEach(call => call.resolve())
  await tick()
  assert.equal(calls.length, 7)
  assert.equal(state.items[0].status, 'failed')
  assert.ok(state.items[0].file)
  queue.retryFailed()
  await tick()
  assert.equal(calls.length, 7)
  queue.resume()
  await tick()
  assert.equal(calls.length, 9)
  calls.slice(7).forEach(call => call.resolve())
  await tick()
  assert.equal(state.items.every(item => item.status === 'done'), true)
  queue.clearDone()
  assert.equal(state.items.length, 0)
})

test('remove cannot abort an active request, queued removal never reaches the server', async () => {
  const { state, calls, queue } = fixture()
  queue.enqueue(files(8), album)
  queue.remove(1)
  queue.remove(8)
  await tick()
  assert.equal(state.items.length, 7)
  assert.equal(calls.length, 7)
  calls.forEach(call => call.resolve())
  await tick()
  assert.equal(calls.length, 7)
  assert.equal(state.items.length, 7)
})

test('file album destination is captured when enqueued, not after navigation', async () => {
  const { state, calls, queue } = fixture()
  const destination = { id: 'first', name: 'First' }
  queue.pause()
  queue.enqueue(files(1), destination)
  destination.id = 'second'; destination.name = 'Second'
  queue.resume()
  await tick()
  assert.equal(calls[0].albumId, 'first')
  assert.equal(state.items[0].albumName, 'First')
  calls[0].resolve()
  await tick()
})

test('automatic dates remain null and all metadata can be saved as one patch', () => {
  const draft = albumDraftOf({ name: '相册', description: '', displayCreatedDate: null, photoDateStart: null, photoDateEnd: null })
  assert.equal(validateAlbumDraft(draft), null)
  assert.equal(draft.displayCreatedDate, null)
  assert.ok(validateAlbumDraft({ ...draft, photoDateStart: '2026-01-01' }))
  assert.ok(validateAlbumDraft({ ...draft, photoDateStart: '2026-02-01', photoDateEnd: '2026-01-01' }))
  assert.equal(validateAlbumDraft({ ...draft, displayCreatedDate: '2024-01-01', photoDateStart: '2026-01-01', photoDateEnd: '2026-02-01' }), null)
  assert.ok(validateAlbumDraft({ ...draft, name: ' ' }))
  assert.ok(validateAlbumDraft({ ...draft, description: '图'.repeat(1001) }))
})

test('page selection preserves other pages, filtered selection does not include hidden photos', () => {
  assert.deepEqual(toggleVisibleSelection(['a', 'b'], ['b', 'c'], true), ['a', 'b', 'c'])
  assert.deepEqual(toggleVisibleSelection(['a', 'b', 'c'], ['b', 'c'], false), ['a'])
  assert.deepEqual(toggleVisibleSelection([], ['visible'], true), ['visible'])
})
