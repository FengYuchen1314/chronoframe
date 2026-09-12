import type { ReactNode } from 'react'

/**
 * 页头。对应 Vue 侧的 app/components/dashboard/PageHeader.vue：
 * 左标题 + 描述，右操作区。窄屏自动堆叠（见 index.css 的 .admin-page-header）。
 */
export default function PageHeader({
  title,
  description,
  actions,
}: {
  title: string
  description?: string
  actions?: ReactNode
}) {
  return (
    <header className="admin-page-header">
      <div style={{ minWidth: 0 }}>
        <h1
          style={{
            margin: 0,
            fontSize: 24,
            fontWeight: 600,
            lineHeight: 1.4,
            color: 'var(--foreground)',
          }}
        >
          {title}
        </h1>
        {description ? (
          <p style={{ margin: '8px 0 0', color: 'var(--muted)', fontSize: 14, lineHeight: 1.6 }}>
            {description}
          </p>
        ) : null}
      </div>
      {actions ? <div className="admin-page-actions">{actions}</div> : null}
    </header>
  )
}
