import test from 'node:test'
import assert from 'node:assert/strict'
import { downloadAlbumSequence, isTouchAlbumDownloadDevice } from '../shared/utils/albumPhotoDownload.ts'

test('phones and tablets, including desktop-UA iPads, use individual downloads', () => {
  assert.equal(isTouchAlbumDownloadDevice('iPhone', 5, false, false), true)
  assert.equal(isTouchAlbumDownloadDevice('Android Tablet', 5, false, false), true)
  assert.equal(isTouchAlbumDownloadDevice('Macintosh', 5, false, false), true)
  assert.equal(isTouchAlbumDownloadDevice('Macintosh', 0, false, false), false)
  assert.equal(isTouchAlbumDownloadDevice('Windows', 10, false, false), false)
  assert.equal(isTouchAlbumDownloadDevice('Windows', 10, true, false), true)
  assert.equal(isTouchAlbumDownloadDevice('Windows', 0, false, true), true)
})

test('downloads are sequential, ordered and resume only remaining images', async () => {
  const saved = [], progress = []
  let inflight = 0
  const options = { start: 1, signal: new AbortController().signal,
    async load(item) { assert.equal(++inflight, 1); await Promise.resolve(); inflight--; return item },
    save(body) { saved.push(body) }, progress(count) { progress.push(count) } }
  await downloadAlbumSequence([0, 1, 2, 3], options)
  assert.deepEqual(saved, [1, 2, 3])
  assert.deepEqual(progress, [2, 3, 4])
})

test('a late fetch completion after cancellation cannot save or start the next image', async () => {
  const controller = new AbortController(), loaded = [], saved = []
  await assert.rejects(downloadAlbumSequence([1, 2], { start: 0, signal: controller.signal,
    async load(item) { loaded.push(item); controller.abort(); return item },
    save(body) { saved.push(body) }, progress() { assert.fail('cancelled response counted') } }))
  assert.deepEqual(loaded, [1])
  assert.deepEqual(saved, [])
})

test('failure stops the sequence, and retry never repeats completed images', async () => {
  let completed = 0
  const saved = []
  const options = { start: 0, signal: new AbortController().signal,
    async load(item) { if (item === 2) throw new Error('network'); return item },
    save(body) { saved.push(body) }, progress(count) { completed = count } }
  await assert.rejects(downloadAlbumSequence([1, 2, 3], options), /network/)
  assert.equal(completed, 1)
  await downloadAlbumSequence([1, 2, 3], { ...options, start: completed, load: async item => item })
  assert.deepEqual(saved, [1, 2, 3])
  assert.equal(completed, 3)
})

test('Stop between files and an already-aborted request dispatch no further downloads', async () => {
  const controller = new AbortController(), saved = []
  const options = { start: 0, signal: controller.signal, load: async item => item,
    save(body) { saved.push(body) }, progress() {}, pause: async () => controller.abort() }
  await assert.rejects(downloadAlbumSequence([1, 2], options))
  assert.deepEqual(saved, [1])
  await assert.rejects(downloadAlbumSequence([3], options))
  assert.deepEqual(saved, [1])
})
