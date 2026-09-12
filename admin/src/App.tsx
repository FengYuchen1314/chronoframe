import { lazy } from 'react'
import { createBrowserRouter, Navigate, RouterProvider } from 'react-router-dom'
import { Toast } from '@heroui/react'
import { ConfirmHost } from './lib/notice'
import AppShell from './components/AppShell'

// 页面按路由懒加载。HeroUI 与 React Aria 的体量决定了单个 chunk 偏大，
// 而六个页面里任何一次访问只会用到其中一个，因此把首屏压到外壳 + 概览。
// 加载期间的占位由 AppShell 在 <Outlet /> 外层提供，侧栏与顶栏保持可用。
const Overview = lazy(() => import('./pages/Overview'))
const Albums = lazy(() => import('./pages/Albums'))
const Downloads = lazy(() => import('./pages/Downloads'))
const Tasks = lazy(() => import('./pages/Tasks'))
const SettingsGeneral = lazy(() => import('./pages/SettingsGeneral'))
const SettingsStorage = lazy(() => import('./pages/SettingsStorage'))

// 使用 data router 而不是 <BrowserRouter>：相册页与设置页需要 useBlocker
// 来实现「有未保存修改时阻止离开」，那个 API 只在 data router 下可用。
// basename 必须与 Vite 的 base 一致，否则刷新 /dashboard/albums 这类深链会对不上路由。
const router = createBrowserRouter(
  [
    {
      element: <AppShell />,
      children: [
        { index: true, element: <Overview /> },
        { path: 'albums', element: <Albums /> },
        { path: 'downloads', element: <Downloads /> },
        { path: 'tasks', element: <Tasks /> },
        {
          path: 'settings',
          children: [
            // 默认进「存储与维护」，与现网进入设置的行为一致。
            { index: true, element: <Navigate to="storage" replace /> },
            { path: 'general', element: <SettingsGeneral /> },
            { path: 'storage', element: <SettingsStorage /> },
          ],
        },
        // 格式转换的前端已在 fbe6436 移除，现网此路由是重定向；保留同样行为。
        { path: 'conversions', element: <Navigate to="/settings/storage" replace /> },
        { path: '*', element: <Navigate to="/" replace /> },
      ],
    },
  ],
  { basename: '/dashboard' },
)

export default function App() {
  return (
    <>
      <Toast.Provider placement="top end" maxVisibleToasts={4} />
      <ConfirmHost />
      <RouterProvider router={router} />
    </>
  )
}
