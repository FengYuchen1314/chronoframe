import { returnTransform, VIEWER_RETURN_DURATION } from '~~/shared/utils/viewerPerformance'
import { animationDeadline } from './animationDeadline'

const containedImageRect = (image: HTMLImageElement) => {
  const bounds = image.getBoundingClientRect()
  if (!image.naturalWidth || !image.naturalHeight) return bounds
  const ratio = Math.min(bounds.width / image.naturalWidth, bounds.height / image.naturalHeight)
  const width = image.naturalWidth * ratio
  const height = image.naturalHeight * ratio
  return { left: bounds.left + (bounds.width - width) / 2, top: bounds.top + (bounds.height - height) / 2, width, height }
}

/** One compositor animation over the still-mounted gallery; no layout tween or polling. */
export function createViewerReturnAnimation(photoId: string) {
  const root = document.querySelector<HTMLElement>('[data-viewer-current="true"]')
  const images = root ? Array.from(root.querySelectorAll<HTMLImageElement>('img')) : []
  const source = images.find(image => image.hasAttribute('data-progressive-full') && image.complete && image.naturalWidth > 0 && getComputedStyle(image).opacity !== '0')
    || images.find(image => image.hasAttribute('data-progressive-placeholder') && image.complete && image.naturalWidth > 0)
  const target = document.querySelector<HTMLElement>(`[data-photo-id="${CSS.escape(photoId)}"]`)
  if (!source || document.hidden || window.matchMedia('(prefers-reduced-motion: reduce)').matches) return null

  const sourceRect = containedImageRect(source)
  let targetRect = target?.getBoundingClientRect()
  if (target && targetRect && (targetRect.bottom < 32 || targetRect.top > window.innerHeight - 32)) {
    // A user may have swiped many photos away. Move the background once, while
    // it is covered, instead of flying to an off-screen stale snapshot.
    target.scrollIntoView({ block: 'center', behavior: 'instant' })
    targetRect = target.getBoundingClientRect()
  }
  const transform = targetRect ? returnTransform(sourceRect, targetRect) : null
  const clone = document.createElement('img')
  clone.src = source.currentSrc || source.src
  clone.alt = ''
  clone.setAttribute('aria-hidden', 'true')
  clone.dataset.viewerReturn = 'true'
  Object.assign(clone.style, {
    position: 'fixed', zIndex: '110', pointerEvents: 'none', objectFit: 'fill',
    top: `${sourceRect.top}px`, left: `${sourceRect.left}px`,
    width: `${sourceRect.width}px`, height: `${sourceRect.height}px`,
    transformOrigin: '0 0', willChange: 'transform,opacity',
  })
  document.body.appendChild(clone)
  const previousVisibility = target?.style.visibility || ''
  if (target && transform) target.style.visibility = 'hidden'
  const animation = clone.animate(
    transform
      ? [{ transform: 'translate3d(0,0,0) scale(1)', opacity: 1 }, { transform, opacity: 1 }]
      : [{ transform: 'scale(1)', opacity: 1 }, { transform: 'scale(.96)', opacity: 0 }],
    { duration: transform ? VIEWER_RETURN_DURATION : 160, easing: 'cubic-bezier(.2,.75,.25,1)', fill: 'forwards' },
  )
  return {
    // Never make leaving the viewer depend indefinitely on compositor events.
    finished: animationDeadline(animation.finished),
    cleanup: () => {
      animation.cancel()
      clone.remove()
      if (target) target.style.visibility = previousVisibility
    },
  }
}
