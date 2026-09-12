import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// 管理后台是独立构建的 React SPA，产出到 admin/dist，再由构建脚本落到
// public/dashboard，最终随 Nuxt 静态产物一起发布到 /dashboard/。
// 独立构建的目的：HeroUI 与前台使用的 @nuxt/ui 都是 Tailwind v4 基座，
// 放在同一个构建里会产生全局样式互相渗透，独立构建可保证前台样式零改动。
export default defineConfig({
  base: '/dashboard/',
  plugins: [react(), tailwindcss()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    sourcemap: false,
  },
  server: {
    port: 5174,
    // 开发时后端 Rust API 独立运行在 8080，直接代理过去以便同源携带 Cookie。
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: false,
      },
    },
  },
})
