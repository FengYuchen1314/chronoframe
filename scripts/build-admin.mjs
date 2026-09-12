// 构建管理后台（独立 React SPA）并把产物落到 public/dashboard。
//
// 为什么要独立构建：后台用 HeroUI、前台用 @nuxt/ui，两者都是 Tailwind v4 基座。
// 放进同一个构建里会产生全局样式互相渗透（前台的 preflight、主题变量、工具类都可能
// 被对方的配置改写）。独立构建让两套 CSS 完全隔离，前台可以做到零改动。
//
// 产物落点 public/dashboard 会被 `nuxt generate` 原样复制到 .output/public/dashboard，
// 最终由 Rust 服务的静态托管在 /dashboard/ 路径下提供。
import { spawnSync } from 'node:child_process'
import { cp, rm, access } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const adminDir = resolve(root, 'admin')
const distDir = resolve(adminDir, 'dist')
const targetDir = resolve(root, 'public/dashboard')

const run = (command, args, cwd) => {
  const result = spawnSync(command, args, { cwd, stdio: 'inherit', shell: false })
  if (result.status !== 0) {
    console.error(`[build-admin] 命令失败：${command} ${args.join(' ')}`)
    process.exit(result.status ?? 1)
  }
}

const ensureDependencies = async () => {
  if (existsSync(resolve(adminDir, 'node_modules'))) return
  console.log('[build-admin] admin/ 缺少依赖，先执行 pnpm install')
  run('pnpm', ['install'], adminDir)
}

const main = async () => {
  await access(resolve(adminDir, 'package.json')).catch(() => {
    console.error('[build-admin] 找不到 admin/package.json')
    process.exit(1)
  })

  await ensureDependencies()
  run('pnpm', ['run', 'build'], adminDir)

  if (!existsSync(distDir)) {
    console.error('[build-admin] 构建结束但未找到 admin/dist')
    process.exit(1)
  }

  // 先清空再复制：避免上一次构建残留的带哈希资源被一起发布。
  await rm(targetDir, { recursive: true, force: true })
  await cp(distDir, targetDir, { recursive: true })
  console.log(`[build-admin] 已输出到 ${targetDir}`)
}

await main()
