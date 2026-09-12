import { Button } from '@heroui/react'
import { Icon } from '../lib/icons'

/** 前端分页控件。用于相册列表、图片网格、任务表等小数据集。 */
export default function Pager({
  page,
  pageSize,
  total,
  onChange,
  disabled = false,
}: {
  page: number
  pageSize: number
  total: number
  onChange: (page: number) => void
  disabled?: boolean
}) {
  const pageCount = Math.max(1, Math.ceil(total / pageSize))
  if (pageCount <= 1) return null

  // 只渲染当前页附近的页码，避免页数很多时把工具条撑爆。
  const windowSize = 5
  let start = Math.max(1, page - Math.floor(windowSize / 2))
  const end = Math.min(pageCount, start + windowSize - 1)
  start = Math.max(1, end - windowSize + 1)
  const pages: number[] = []
  for (let index = start; index <= end; index += 1) pages.push(index)

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
      <Button
        variant="ghost"
        size="sm"
        isIconOnly
        aria-label="上一页"
        isDisabled={disabled || page <= 1}
        onPress={() => onChange(page - 1)}
      >
        <Icon name="chevron-left" size={16} />
      </Button>
      {start > 1 ? <span className="admin-help">…</span> : null}
      {pages.map((value) => (
        <Button
          key={value}
          variant={value === page ? 'primary' : 'ghost'}
          size="sm"
          isDisabled={disabled}
          onPress={() => onChange(value)}
        >
          {value}
        </Button>
      ))}
      {end < pageCount ? <span className="admin-help">…</span> : null}
      <Button
        variant="ghost"
        size="sm"
        isIconOnly
        aria-label="下一页"
        isDisabled={disabled || page >= pageCount}
        onPress={() => onChange(page + 1)}
      >
        <Icon name="chevron-right" size={16} />
      </Button>
    </div>
  )
}
