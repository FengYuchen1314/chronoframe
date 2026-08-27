import test from 'node:test'
import assert from 'node:assert/strict'
import { returnTransform, thumbnailWindow, viewerNeighborIndices, VIEWER_RETURN_DURATION } from '../shared/utils/viewerPerformance.ts'
import { createViewerPreloader } from '../shared/utils/viewerPreloader.ts'
import { masonryLayout } from '../shared/utils/masonryLayout.ts'

const photos = Array.from({ length: 1000 }, (_, id) => ({ id: `${id}`, previewUrl: `/photos/${id}/preview` }))
const harness = () => {
  const images = []
  let ready = new Set()
  const preloader = createViewerPreloader(ids => { ready = ids }, () => {
    const image = {
      src: '', onload: null, onerror: null, cancelled: false, done: false,
      removeAttribute(name) { assert.equal(name, 'src'); this.cancelled = true },
      finish(success = true) { this.done = true; (success ? this.onload : this.onerror)?.() },
    }
    images.push(image)
    return image
  })
  return { preloader, images, ready: () => ready, active: () => images.filter(image => !image.cancelled && !image.done) }
}

test('masonry positions all photos in one pass using stored dimensions', () => {
  assert.deepEqual(masonryLayout([1, 1, 1, 1], 564), [[0, 2], [1, 3]])
  assert.deepEqual(masonryLayout([1, 1, 1], 564, 280, 4, 2, 8, 900), [[], [0, 1, 2]])
  assert.deepEqual(masonryLayout([1], 0), [])
  const columns = masonryLayout(Array.from({ length: 10000 }, (_, i) => i % 2 ? 0.6 : 1.7), 1280)
  assert.equal(columns.length, 4)
  const indices = columns.flat()
  assert.equal(indices.length, 10000)
  assert.equal(new Set(indices).size, 10000)
  assert.ok(columns.every(column => column.length > 0))
})

test('return is a single bounded transform, never a layout-property tween', () => {
  assert.equal(returnTransform({ left: 10, top: 20, width: 1000, height: 500 }, { left: 50, top: 60, width: 200, height: 100 }), 'translate3d(40px, 40px, 0) scale(0.2, 0.2)')
  assert.equal(returnTransform({ left: 0, top: 0, width: 0, height: 1 }, { left: 0, top: 0, width: 2, height: 2 }), null)
  assert.equal(returnTransform({ left: NaN, top: 0, width: 1, height: 1 }, { left: 0, top: 0, width: 2, height: 2 }), null)
  assert.ok(VIEWER_RETURN_DURATION <= 250)
})

test('thumbnail strip work is bounded by the viewport, not album length', () => {
  for (const count of [100, 1000, 10000]) {
    const { start, end } = thumbnailWindow(500, 1280, count, 76)
    assert.ok(end - start <= 24)
    assert.ok(start >= 0 && end <= count)
  }
  assert.deepEqual(viewerNeighborIndices(0, 3), [1, 2])
  assert.deepEqual(viewerNeighborIndices(2, 3), [1, 0])
})

test('current image starts alone; neighbors use two low-priority preview requests', () => {
  const h = harness()
  h.preloader.setWindow(photos, 5)
  assert.equal(h.images.length, 0)
  h.preloader.markReady('5')
  assert.deepEqual(h.images.map(i => i.src), ['/photos/6/preview', '/photos/4/preview'])
  assert.ok(h.images.every(i => i.fetchPriority === 'low'))
  h.images[0].finish()
  assert.equal(h.images[2].src, '/photos/7/preview')
  assert.equal(h.active().length, 2)
})

test('swiping promotes the existing request instead of aborting and restarting it', () => {
  const h = harness()
  h.preloader.setWindow(photos, 5)
  h.preloader.markReady('5')
  const next = h.images[0]
  h.preloader.setWindow(photos, 6)
  assert.equal(next.cancelled, false)
  assert.equal(next.fetchPriority, 'high')
  assert.equal(h.images.filter(i => i.src === next.src).length, 1)
  next.finish()
  assert.ok(h.ready().has('6'))
})

test('stale completion after cancellation cannot mutate the new window', () => {
  const h = harness()
  h.preloader.setWindow(photos, 5)
  h.preloader.markReady('5')
  const stale = h.images[0].onload
  h.preloader.setWindow(photos, 30)
  assert.ok(h.images.every(i => i.cancelled))
  stale()
  assert.equal(h.ready().size, 0)
  assert.equal(h.active().length, 0)
  h.preloader.markReady('30')
  assert.equal(h.active().length, 2)
  h.preloader.clear()
  assert.equal(h.active().length, 0)
  assert.equal(h.ready().size, 0)
})

test('hundreds of switches remain bounded and never fetch high/original files', () => {
  const h = harness()
  for (let index = 0; index < 300; index++) {
    h.preloader.setWindow(photos, index)
    h.preloader.markReady(`${index}`)
    for (let n = 0; n < 4; n++) h.active()[0]?.finish()
    assert.ok(h.active().length <= 2)
    assert.ok(h.ready().size <= 5)
  }
  assert.ok(h.images.every(i => i.src.endsWith('/preview')))
  h.preloader.clear()
})

test('one failed neighbor does not block the rest or retry in a loop', () => {
  const h = harness()
  h.preloader.setWindow(photos, 10)
  h.preloader.markReady('10')
  h.images[0].finish(false)
  assert.equal(h.images.length, 3)
  assert.equal(h.images.filter(i => i.src === '/photos/11/preview').length, 1)
})
