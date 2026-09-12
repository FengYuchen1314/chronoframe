import type { ReactNode } from 'react'
import { Checkbox, Spinner } from '@heroui/react'

export interface Column<T> {
  key: string
  title: string
  width?: number
  /** 单元格渲染。index 是当前页内的行序号（用于「顺序」列）。 */
  render: (row: T, index: number) => ReactNode
  align?: 'left' | 'right' | 'center'
}

interface DataTableProps<T> {
  columns: Column<T>[]
  rows: T[]
  rowKey: (row: T) => string
  loading?: boolean
  emptyText?: string
  /** 传入即启用勾选列。相册/图片的跨页保留选择由调用方用 string[] 自行维护。 */
  selection?: {
    selectedKeys: string[]
    onChange: (keys: string[]) => void
    disabled?: boolean
  }
  minWidth?: number
}

/**
 * 轻量数据表格。
 *
 * 说明：HeroUI v3 的 Table 是 React Aria 的 Collection 复合结构，用于服务端虚拟化
 * 等场景；后台这里的表格是固定列、前端分页的小数据集，用语义化 table + 主题变量
 * 实现更直接，也更容易保证列宽/横向滚动的行为与现网一致。
 */
export default function DataTable<T>({
  columns,
  rows,
  rowKey,
  loading = false,
  emptyText = '暂无数据',
  selection,
  minWidth,
}: DataTableProps<T>) {
  const allKeys = rows.map(rowKey)
  const selectedSet = new Set(selection?.selectedKeys ?? [])
  const allSelected = allKeys.length > 0 && allKeys.every((key) => selectedSet.has(key))
  const someSelected = allKeys.some((key) => selectedSet.has(key))

  const toggleAll = (checked: boolean) => {
    if (!selection) return
    const next = new Set(selection.selectedKeys)
    for (const key of allKeys) {
      if (checked) next.add(key)
      else next.delete(key)
    }
    selection.onChange([...next])
  }

  const toggleRow = (key: string, checked: boolean) => {
    if (!selection) return
    const next = new Set(selection.selectedKeys)
    if (checked) next.add(key)
    else next.delete(key)
    selection.onChange([...next])
  }

  const computedMinWidth = minWidth
    ?? (columns.reduce((total, column) => total + (column.width ?? 160), 0)
      + (selection ? 48 : 0))

  return (
    <div style={{ overflowX: 'auto', opacity: loading ? 0.7 : 1 }}>
      <table
        style={{
          width: '100%',
          minWidth: computedMinWidth,
          borderCollapse: 'collapse',
          fontSize: 14,
        }}
      >
        <thead>
          <tr style={{ borderBottom: '1px solid var(--border)' }}>
            {selection ? (
              <th style={{ width: 48, padding: '10px 8px', textAlign: 'left' }}>
                <Checkbox
                  aria-label="全选本页"
                  isSelected={allSelected}
                  isIndeterminate={!allSelected && someSelected}
                  isDisabled={selection.disabled || allKeys.length === 0}
                  onChange={toggleAll}
                />
              </th>
            ) : null}
            {columns.map((column) => (
              <th
                key={column.key}
                style={{
                  width: column.width,
                  padding: '10px 12px',
                  textAlign: column.align ?? 'left',
                  fontWeight: 600,
                  color: 'var(--muted)',
                  fontSize: 13,
                  whiteSpace: 'nowrap',
                }}
              >
                {column.title}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => {
            const key = rowKey(row)
            return (
              <tr key={key} style={{ borderBottom: '1px solid var(--separator)' }}>
                {selection ? (
                  <td style={{ padding: '10px 8px' }}>
                    <Checkbox
                      aria-label="选择此行"
                      isSelected={selectedSet.has(key)}
                      isDisabled={selection.disabled}
                      onChange={(checked: boolean) => toggleRow(key, checked)}
                    />
                  </td>
                ) : null}
                {columns.map((column) => (
                  <td
                    key={column.key}
                    style={{
                      padding: '10px 12px',
                      textAlign: column.align ?? 'left',
                      verticalAlign: 'middle',
                    }}
                  >
                    {column.render(row, index)}
                  </td>
                ))}
              </tr>
            )
          })}
        </tbody>
      </table>

      {!rows.length ? (
        <div
          style={{
            display: 'grid',
            placeItems: 'center',
            gap: 8,
            padding: '40px 0',
            color: 'var(--muted)',
            fontSize: 14,
          }}
        >
          {loading ? <Spinner size="sm" /> : null}
          <span>{loading ? '加载中…' : emptyText}</span>
        </div>
      ) : null}
    </div>
  )
}
