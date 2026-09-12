// 从 @iconify-json/tabler 官方图标数据中提取后台实际用到的图标，
// 生成自包含的 React 组件模块。这样做而不是用 @iconify/react 在线模式，
// 是因为本产品是自托管应用，不能依赖运行时访问 Iconify API；
// 也不整包引入 tabler（4MB+），只保留需要的几十个。
//
// 重新生成：node scripts/gen-icons.mjs
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..')

// 只保留后台源码里**实际引用**的图标（27 个）。
// Tabler 每个图标的 SVG 路径都不短，多声明一个就多几 KB，
// 早先按"可能用得上"列了 69 个，产物里的图标分包因此膨胀到 228 KB。
// 需要新图标时：把名字加到这里，然后 `node scripts/gen-icons.mjs` 重新生成。
const NAMES = [
  // 侧栏导航
  'dashboard', 'album', 'download', 'activity', 'settings', 'database',
  // 顶栏与布局
  'menu-2', 'chevron-right', 'chevron-left', 'external-link',
  // 主题选项
  'sun', 'moon', 'device-desktop',
  // 通用动作
  'plus', 'x', 'refresh', 'check', 'arrows-sort', 'arrow-up', 'arrow-down',
  // 上传与相册
  'upload', 'cloud-upload', 'photo-plus',
  // 下载与空态
  'file-zip', 'inbox',
  // 上传队列的暂停 / 继续
  'player-pause', 'player-play',
]

const collection = JSON.parse(
  readFileSync(resolve(root, 'node_modules/@iconify-json/tabler/icons.json'), 'utf8'),
)

const pick = (name) => {
  const entry = collection.icons[name]
  if (!entry) return undefined
  if (typeof entry === 'string') return { body: entry }
  return {
    body: entry.body,
    width: entry.width ?? collection.width ?? 24,
    height: entry.height ?? collection.height ?? 24,
  }
}

const toPascal = (name) =>
  name.split('-').map((part) => part.charAt(0).toUpperCase() + part.slice(1)).join('')

const missing = []
const entries = NAMES.map((name) => {
  const data = pick(name)
  if (!data) missing.push(name)
  return [name, data]
}).filter(([, data]) => data)

if (missing.length) {
  console.error(`[gen-icons] 以下图标在 tabler 集合中不存在，已跳过：${missing.join(', ')}`)
  process.exitCode = 1
}

const header = `/* eslint-disable */
// 本文件由 scripts/gen-icons.mjs 自动生成，请勿手工编辑。
// 图标来源：Tabler Icons（MIT）· @iconify-json/tabler
import type { ComponentType, SVGProps } from 'react'

export type IconProps = SVGProps<SVGSVGElement> & { size?: number | string }

const base = {
  xmlns: 'http://www.w3.org/2000/svg',
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
} as const
`

const components = entries.map(([name, data]) => {
  const component = `Icon${toPascal(name)}`
  const body = JSON.stringify(data.body)
  const size = data.width === data.height ? '' : ` width={${data.width}} height={${data.height}}`
  return `export const ${component} = ({ size = 24, width, height, ...rest }: IconProps) => (\n  <svg {...base}${size} width={width ?? size} height={height ?? size} {...rest} dangerouslySetInnerHTML={{ __html: ${body} }} />\n)`
}).join('\n\n')

const mapEntries = entries
  .map(([name]) => `  '${name}': Icon${toPascal(name)},`)
  .join('\n')

const footer = `
/** 图标名 -> 组件。同时支持 \`album\` 与 \`tabler:album\` 两种写法。 */
const registry = {
${mapEntries}
} as const

export type IconName = keyof typeof registry

const resolveIcon = (name: string) => {
  const key = name.startsWith('tabler:') ? name.slice('tabler:'.length) : name
  return (registry as Record<string, ComponentType<IconProps>>)[key]
}

/**
 * 按名字渲染图标。用法与 Vue 侧的 \`<Icon name="tabler:album" />\` 保持一致，
 * 便于逐行对照迁移。未知图标名不抛错、渲染为空，避免一个拼写错误让整页崩掉；
 * 但会在开发构建里发一条告警，否则图标会静默消失、很难定位。
 */
export const Icon = ({ name, ...rest }: { name: string } & IconProps) => {
  const Component = resolveIcon(name)
  if (!Component) {
    if (import.meta.env.DEV) {
      console.warn(\`[icons] 未收录的图标 "\${name}"。请把它加入 admin/scripts/gen-icons.mjs 的 NAMES 并重新生成。\`)
    }
    return null
  }
  return <Component {...rest} />
}

export const hasIcon = (name: string) => Boolean(resolveIcon(name))
`

mkdirSync(resolve(root, 'src/lib'), { recursive: true })
writeFileSync(
  resolve(root, 'src/lib/icons.tsx'),
  `${header}\n${components}\n${footer}`,
  'utf8',
)
console.log(`[gen-icons] 已生成 ${entries.length} 个图标 -> src/lib/icons.tsx`)
