import { useEffect, useRef } from 'react'
import { useBlocker } from 'react-router-dom'
import { notice } from './notice'

/**
 * 有未保存修改时阻止离开当前页面（对应现网的 onBeforeRouteLeave）。
 * 条件为 true 时，页面内的任何导航都会先弹后台自己的确认框。
 */
export function useLeaveConfirm(active: boolean, message: string) {
  useLeaveGuard({
    shouldBlock: () => active,
    message,
  })
}

/**
 * 可自定义条件的离开守卫。
 *
 * 相册页需要更细的语义：只有 `?album` 变化（切换相册或离开工作区）才拦截，
 * 仅在同一相册内切换页签不拦截；而"正在提交"时是完全拒绝离开并给出提示，
 * 不是询问。这两种行为都在这里统一处理。
 */
export function useLeaveGuard({
  shouldBlock,
  message,
  hardBlock = false,
  hardBlockMessage = '正在提交操作，请稍候',
}: {
  shouldBlock: (next: { pathname: string, search: string }, current: { pathname: string, search: string }) => boolean
  message: string
  /** true 时不询问，直接拒绝离开并提示（用于请求提交中）。 */
  hardBlock?: boolean
  hardBlockMessage?: string
}) {
  const blocker = useBlocker(({ currentLocation, nextLocation }) =>
    shouldBlock(
      { pathname: nextLocation.pathname, search: nextLocation.search },
      { pathname: currentLocation.pathname, search: currentLocation.search },
    ))

  // 每次被拦截的目标只处理一次。React 的 StrictMode 会重复执行 effect，
  // 没有这个守卫时同一个确认框会被弹两次，第一个 Promise 被以 false 收尾，
  // 会把已经 reset 的 blocker 再 proceed 一次。
  const handled = useRef<string | null>(null)

  useEffect(() => {
    if (blocker.state !== 'blocked') {
      handled.current = null
      return
    }
    const key = `${blocker.location.pathname}${blocker.location.search}`
    if (handled.current === key) return
    handled.current = key

    if (hardBlock) {
      notice.add({ title: hardBlockMessage, color: 'warning' })
      blocker.reset()
      return
    }
    void notice.confirm(message).then((confirmed) => {
      if (confirmed) blocker.proceed()
      else blocker.reset()
    })
    // blocker 每次状态变化都是新对象，用它本身作为依赖即可。
  }, [blocker, hardBlock, hardBlockMessage, message])
}

/** 刷新/关闭标签页时的原生确认。现网用 preventDefault() + returnValue。 */
export function useBeforeUnload(active: boolean) {
  useEffect(() => {
    if (!active) return
    const handler = (event: BeforeUnloadEvent) => {
      event.preventDefault()
      event.returnValue = ''
    }
    window.addEventListener('beforeunload', handler)
    return () => window.removeEventListener('beforeunload', handler)
  }, [active])
}
