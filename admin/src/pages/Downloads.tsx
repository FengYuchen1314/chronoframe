import DownloadManager from '../components/DownloadManager'
import { useDocumentTitle } from '../lib/hooks'

/**
 * 下载管理独立页。对应 Vue 侧的 app/pages/dashboard/downloads.vue，
 * 那里只是一个 6 行外壳，真正的 UI 全在 DownloadManager 里。
 * 相册来源是 URL query（?album=），因此这里不传 albumId。
 */
export default function Downloads() {
  useDocumentTitle('下载管理')
  return <DownloadManager />
}
