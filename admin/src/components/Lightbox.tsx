// 图片预览浮层（自研 Lightbox）。
//
// 对应 Vue 侧 ant-design-vue 的 `AImage :preview="{ src }"`：缩略图用
// `/api/photos/{id}/thumbnail?v=grid2`，点开后加载 `/api/photos/{id}/preview`
// 的预览图（spec-albums.md:89,97,692）。HeroUI 没有等价组件，所以自己实现。
//
// 行为对齐 antd 的图片预览：ESC 关闭、点遮罩关闭、右上角关闭按钮，
// 预览图加载期间显示占位文案而不是空白。
import { useEffect, useRef, useState } from 'react'
import { Icon } from '../lib/icons'

export interface LightboxPhoto {
  id: string
  originalName: string
}

export default function Lightbox({
  photo,
  onClose,
}: {
  photo: LightboxPhoto | null
  onClose: () => void
}) {
  const closeButton = useRef<HTMLButtonElement | null>(null)
  // 以 photo.id 为键重置加载态：连续预览不同图片时不能沿用上一张的"已加载"。
  const [loaded, setLoaded] = useState(false)

  useEffect(() => { setLoaded(false) }, [photo?.id])

  // onClose 存进 ref，下面的 effect 才能只依赖「开了哪张图」。
  // 调用方传的是内联箭头函数，每次渲染都是新身份；把它放进依赖里，父组件
  // 每次重渲染（上传队列推进时很频繁）都会重跑 effect —— 反复抢走焦点。
  const closeRef = useRef(onClose)
  closeRef.current = onClose

  const photoId = photo?.id
  useEffect(() => {
    if (!photoId) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation()
        closeRef.current()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    // 浮层打开期间锁滚动，否则背后的图片网格会跟着滚。
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    closeButton.current?.focus()
    return () => {
      window.removeEventListener('keydown', onKeyDown)
      document.body.style.overflow = previousOverflow
    }
  }, [photoId])

  if (!photo) return null

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`预览图片：${photo.originalName}`}
      onClick={onClose}
      style={{
        position: 'fixed',
        inset: 0,
        // 高于侧栏（z-index:5）与 HeroUI 的遮罩层，确保盖在整页之上。
        zIndex: 100,
        display: 'flex',
        flexDirection: 'column',
        background: 'rgba(0, 0, 0, 0.82)',
      }}
    >
      <div
        style={{
          flexShrink: 0,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 12,
          padding: '12px 16px',
          color: '#fff',
        }}
      >
        <span
          title={photo.originalName}
          style={{
            fontSize: 14,
            minWidth: 0,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {photo.originalName}
        </span>
        <button
          ref={closeButton}
          type="button"
          aria-label="关闭预览"
          onClick={onClose}
          style={{
            flexShrink: 0,
            display: 'inline-flex',
            alignItems: 'center',
            justifyContent: 'center',
            width: 36,
            height: 36,
            borderRadius: 999,
            border: 0,
            background: 'rgba(255, 255, 255, 0.16)',
            color: '#fff',
            cursor: 'pointer',
          }}
        >
          <Icon name="x" size={20} />
        </button>
      </div>

      <div
        style={{
          position: 'relative',
          flex: 1,
          minHeight: 0,
          display: 'grid',
          placeItems: 'center',
          padding: '0 16px 24px',
        }}
      >
        {!loaded ? (
          <span
            style={{
              position: 'absolute',
              inset: 0,
              display: 'grid',
              placeItems: 'center',
              color: 'rgba(255, 255, 255, 0.72)',
              fontSize: 14,
            }}
          >
            正在加载预览…
          </span>
        ) : null}
        <img
          // 点图片本身不关闭：只有遮罩和关闭按钮会关。
          onClick={(event) => event.stopPropagation()}
          src={`/api/photos/${encodeURIComponent(photo.id)}/preview`}
          alt={photo.originalName}
          onLoad={() => setLoaded(true)}
          // 预览图取不到时（例如派生图还没生成）也要让用户看到结果，而不是一直转。
          onError={() => setLoaded(true)}
          style={{
            maxWidth: '100%',
            maxHeight: '100%',
            objectFit: 'contain',
            opacity: loaded ? 1 : 0,
            transition: 'opacity 0.15s',
          }}
        />
      </div>
    </div>
  )
}
