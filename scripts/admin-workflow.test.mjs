import test from 'node:test'
import assert from 'node:assert/strict'
// 管理后台已重写为独立 React SPA，这些纯逻辑模块随之搬到 admin/src/lib 下。
// 测试本身与框架无关，直接改指向新位置继续有效。
import { createUploadQueue, createUploadQueueState } from '../admin/src/lib/upload-queue.ts'
import { albumDraftOf, toggleVisibleSelection, validateAlbumDraft } from '../admin/src/lib/albums.ts'

const tick = () => new Promise(resolve => setImmediate(resolve))
const files = count => Array.from({ length: count }, (_, index) => ({ name: `${index}.png`, size: 123 }))
const album = { id: 'album-a', name: 'A' }
function fixture() {
  const state = createUploadQueueState()
  const calls = []
  // frames 记录每次 notify() 时 UI 能看到的状态。React 侧（admin/src/lib/store.ts）
  // 就是靠这个回调触发重渲染，所以"最后一帧"必须等于队列的真实终态。
  const frames = []
  const snapshot = () => ({
    uploading: state.items.filter(item => item.status === 'uploading').length,
    queued: state.items.filter(item => item.status === 'queued').length,
    done: state.items.filter(item => item.status === 'done').length,
    failed: state.items.filter(item => item.status === 'failed').length,
    albumVersions: { ...state.albumVersions },
  })
  const queue = createUploadQueue(state, (file, albumId) => new Promise((resolve, reject) => calls.push({ file, albumId, resolve, reject })), String, () => frames.push(snapshot()))
  return { state, calls, queue, frames, snapshot }
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

// 回归：终态必须通知到调用方。
// 曾经的写法是在 upload() 的 finally 里通知，那比 'uploading' → 'done' 的赋值早一步，
// 于是队列跑完后 UI 永远停在"上传中"，相册详情页也收不到 albumVersions 的递增。
test('every state transition reaches the subscriber, including the terminal one', async () => {
  const { state, calls, queue, frames, snapshot } = fixture()
  queue.enqueue(files(1), album)
  await tick()
  assert.deepEqual(frames.at(-1), snapshot())
  calls[0].resolve()
  await tick()
  assert.equal(state.items[0].status, 'done')
  assert.deepEqual(frames.at(-1), { uploading: 0, queued: 0, done: 1, failed: 0, albumVersions: { 'album-a': 1 } })
  assert.deepEqual(frames.at(-1), snapshot())
})

test('a failed upload reaches the subscriber too, and never exceeds seven slots mid-flight', async () => {
  const { state, calls, queue, frames, snapshot } = fixture()
  queue.enqueue(files(10), album)
  await tick()
  calls[0].reject(new Error('response lost'))
  await tick()
  assert.equal(state.items[0].status, 'failed')
  assert.equal(frames.at(-1).failed, 1)
  calls.slice(1).forEach(call => call.resolve())
  await tick()
  calls.slice(7).forEach(call => call.resolve())
  await tick()
  // 通知时机的改动绝不能放松并发上限：任何一帧都不能超过 7。
  for (const frame of frames) assert.ok(frame.uploading <= 7, `一帧内并发 ${frame.uploading} 超过 7`)
  assert.deepEqual(frames.at(-1), snapshot())
})

test('pause, resume, retry, remove and clearDone each notify the subscriber', async () => {
  const { calls, queue, frames, snapshot } = fixture()
  queue.enqueue(files(2), album)
  await tick()
  calls[0].reject(new Error('response lost'))
  await tick()
  for (const [label, act] of [
    ['pause', () => queue.pause()],
    ['resume', () => queue.resume()],
    ['retryFailed', () => queue.retryFailed()],
    ['remove', () => queue.remove(2)],
    ['clearDone', () => queue.clearDone()],
  ]) {
    const before = frames.length
    act()
    assert.ok(frames.length > before, `${label}() 没有通知调用方`)
  }
  assert.deepEqual(frames.at(-1), snapshot())
})
