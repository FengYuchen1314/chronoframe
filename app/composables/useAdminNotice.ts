import { App } from 'ant-design-vue'

export function useAdminNotice() {
  const { notification, modal } = App.useApp()
  return {
    add(options: { title: string; description?: string; color?: string }) {
      const type = options.color === 'error' ? 'error' : options.color === 'warning' ? 'warning' : options.color === 'success' ? 'success' : 'info'
      notification[type]({ message: options.title, description: options.description, placement: 'topRight', duration: type === 'error' ? 8 : 4 })
    },
    confirm(content: string, danger = false): Promise<boolean> {
      return new Promise(resolve => modal.confirm({ title: '确认操作', content, okText: '确认', cancelText: '取消', okButtonProps: { danger }, onOk: () => { resolve(true) }, onCancel: () => { resolve(false) } }))
    },
  }
}
