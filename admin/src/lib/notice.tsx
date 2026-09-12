// 通知与确认对话框。
//
// 对应 Vue 侧的 app/composables/useAdminNotice.ts，行为逐条对齐：
//   - add() 的 color -> 通知类型映射：error/warning/success，其余（含 undefined）为中性
//   - error 停留 8 秒，其余 4 秒，右上角，可堆叠
//   - confirm() 标题固定「确认操作」，按钮「确认 / 取消」，返回 Promise<boolean>
//
// 与现网的一处差异是**有意修正**：antd 的 modal.confirm 在组件整体卸载时
// 那个 Promise 可能永远不 settle，调用方会静默挂住。这里在宿主卸载时
// 统一以 false 收尾，避免悬空 Promise。
import { useEffect, useRef, useSyncExternalStore } from 'react'
import { AlertDialog, Button, toast } from '@heroui/react'

export type NoticeColor = 'success' | 'warning' | 'error' | 'info'

export interface NoticeOptions {
  title: string
  description?: string
  color?: NoticeColor | string
}

const variantOf = (color: NoticeOptions['color']) => {
  if (color === 'error') return 'danger' as const
  if (color === 'warning') return 'warning' as const
  if (color === 'success') return 'success' as const
  return 'default' as const
}

export const notice = {
  add({ title, description, color }: NoticeOptions): void {
    const variant = variantOf(color)
    toast(title, {
      description,
      variant,
      // 现网：错误 8 秒，其余 4 秒。
      timeout: variant === 'danger' ? 8000 : 4000,
    })
  },
  confirm(content: string, danger = false): Promise<boolean> {
    return requestConfirm(content, danger)
  },
}

/* ------------------------- 确认对话框的极简 store ------------------------- */

interface ConfirmRequest {
  id: number
  content: string
  danger: boolean
  resolve: (value: boolean) => void
}

let confirmRequest: ConfirmRequest | null = null
let confirmId = 0
const confirmListeners = new Set<() => void>()

const emitConfirm = () => { for (const listener of confirmListeners) listener() }

const requestConfirm = (content: string, danger: boolean): Promise<boolean> => {
  // 同一时刻只保留一个确认框；若已有未决请求，先把旧的按「取消」收尾，
  // 避免它的 Promise 永远悬空。
  confirmRequest?.resolve(false)
  return new Promise<boolean>((resolve) => {
    confirmRequest = { id: ++confirmId, content, danger, resolve }
    emitConfirm()
  })
}

const settleConfirm = (value: boolean) => {
  const current = confirmRequest
  confirmRequest = null
  emitConfirm()
  current?.resolve(value)
}

/** 挂在应用根部的确认对话框宿主。 */
export function ConfirmHost() {
  useSyncExternalStore(
    (listener) => {
      confirmListeners.add(listener)
      return () => { confirmListeners.delete(listener) }
    },
    () => confirmRequest?.id ?? 0,
    () => 0,
  )

  const mounted = useRef(true)
  useEffect(() => () => {
    mounted.current = false
    // 宿主卸载时收尾，避免调用方 await 一个永不 settle 的 Promise。
    confirmRequest?.resolve(false)
    confirmRequest = null
  }, [])

  const current = confirmRequest
  const open = current !== null

  return (
    <AlertDialog.Root
      isOpen={open}
      onOpenChange={(next: boolean) => { if (!next) settleConfirm(false) }}
    >
      <AlertDialog.Backdrop>
        <AlertDialog.Container size="sm">
          <AlertDialog.Dialog>
            <AlertDialog.Header>
              <AlertDialog.Heading>确认操作</AlertDialog.Heading>
            </AlertDialog.Header>
            <AlertDialog.Body>
              <p className="preserve-newlines" style={{ margin: 0, lineHeight: 1.7 }}>
                {current?.content}
              </p>
            </AlertDialog.Body>
            <AlertDialog.Footer>
              <Button variant="ghost" onPress={() => settleConfirm(false)}>取消</Button>
              <Button
                variant={current?.danger ? 'danger' : 'primary'}
                onPress={() => settleConfirm(true)}
              >
                确认
              </Button>
            </AlertDialog.Footer>
          </AlertDialog.Dialog>
        </AlertDialog.Container>
      </AlertDialog.Backdrop>
    </AlertDialog.Root>
  )
}
