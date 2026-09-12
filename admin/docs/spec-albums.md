# 相册管理页 实现规格（Vue 3 / Nuxt 4 → React 重写依据）

> 本文件是「相册管理」页 React 重写的唯一依据，逐字对应当前 Vue 实现的行为契约。
> 所有行号引用指向仓库根目录下的路径。**未修改任何被引用的 Vue/TS 源文件。**

## 阅读范围

| 文件 | 行数 | 说明 |
|---|---|---|
| `app/pages/dashboard/albums.vue` | 310 | 被重写的主页面（下文简称 **albums**） |
| `app/components/dashboard/AlbumCoverEditor.vue` | 127 | 封面编辑器（**COVER**） |
| `app/components/dashboard/MetricCard.vue` | 41 | 复用展示组件（**METRIC**） |
| `app/components/dashboard/PageHeader.vue` | 9 | 后台页头（**HEADER**） |
| `app/components/dashboard/PageHero.vue` | 34 | 复用展示组件（**HERO**） |
| `app/composables/useAdminApi.ts` | 222 | 请求封装 / 鉴权 / CSRF（**API**） |
| `app/types/dashboard.ts` | 193 | 数据类型（**TYPES**） |

为核对真实契约另读了：`shared/utils/admin-albums.ts`、`shared/utils/admin-upload-queue.ts`、`app/composables/useAdminUploads.ts`、`app/composables/useAdminNotice.ts`、`app/utils/adminFormat.ts`、`app/assets/css/admin.css`、`app/layouts/dashboard.vue`、`app/components/dashboard/DownloadManager.vue`、`backend/src/main.rs`、`backend/src/album_covers.rs`、`admin/package.json`、`admin/vite.config.ts`。

**React 目标工程**（已存在骨架）：`admin/` = React 19 + HeroUI `@heroui/react` ^3.2.5 + react-router-dom ^7.18.3 + Tailwind v4 + @iconify/react，`base: '/dashboard/'`，dev 端口 5174、`/api` 代理到 Rust `http://127.0.0.1:8080`（`admin/vite.config.ts:9-26`）。

组件自动导入名（Nuxt 目录前缀规则，`app/components/dashboard/*`）：`PageHeader.vue` → `DashboardPageHeader`，`AlbumCoverEditor.vue` → `DashboardAlbumCoverEditor`，`DownloadManager.vue` → `DashboardDownloadManager`，`UploadQueue.vue` → `DashboardUploadQueue`，`PageHero.vue` → `DashboardPageHero`，`MetricCard.vue` → `DashboardMetricCard`。

---

## 1. 页面结构

页面前置：`definePageMeta({ layout: 'dashboard' })`、`useHead({ title: '相册管理' })`（albums:6-7）。页面由 `selectedId`（来源见 §5）在「列表视图」与「工作区视图」之间二选一。

### 1.1 列表视图（`!selectedId`，albums:242-256）

- `DashboardPageHeader`（albums:237-238）
  - `title` = `相册管理`
  - `description` = `先创建相册，再添加图片。点击相册名称进入管理。`
  - 右侧 actions：`调整顺序`（`:disabled="loading || locked || orderMode || albums.length < 2"`）、`新建相册`（primary，`:disabled="locked || orderMode"`）
- `ACard` → `.admin-toolbar`（flex + wrap + space-between，admin.css:14）
  - 非排序模式左组（albums:244）：`AInputSearch`（`v-model:value="listState.query"`，placeholder `搜索名称或简介`，aria-label `搜索相册`，allow-clear，`style="width:260px;max-width:70vw"`）+ 文本 `{filteredAlbums.length} 个相册`
  - 排序模式左组（albums:245）：粗体 `调整首页展示顺序` + 帮助文本 `上移、下移或置顶，最后统一保存。`
  - 非排序模式右组（albums:246）：`刷新`（`:loading="loading"`）／`导出原始文件（{listState.selected.length}）`（仅 `listState.selected.length` 非 0 时渲染）／`取消选择`（同上条件，动作 `listState.selected = []`）
  - 排序模式右组（albums:247）：`取消`（`:disabled="locked"` → `orderMode = false`）／`保存顺序`（primary，`:disabled="!orderDirty"`，`:loading="mutation === 'order'"`）
- `ATable`（albums:249-255）：`row-key="id"`，`:data-source="filteredAlbums"`，`:loading="loading"`，`:scroll="{ x: 700 }"`
  - **列定义**（albums:62-66）
    - 非排序：`[{title:'相册',key:'album'}, {title:'图片',dataIndex:'photoCount',width:90}, {title:'操作',key:'actions',width:245}]`
    - 排序模式：`[{title:'顺序',key:'order',width:180}, {title:'相册',key:'album'}, {title:'图片',dataIndex:'photoCount',width:90}]`（**排序模式没有「操作」列**）
  - **分页**（albums:249）：`:pagination="orderMode ? false : { current: listState.page, onChange: page => listState.page = page, pageSize: 20, showSizeChanger: false }"`
  - **行选择**（albums:249）：排序模式为 `undefined`；否则 `{ selectedRowKeys: listState.selected, onChange: keys => listState.selected = keys.map(String), preserveSelectedRowKeys: true }`
  - `#bodyCell`（albums:250-254）
    - `order`：`{index + 1}` + `↑`（`:disabled="locked || index === 0"`，aria-label `上移{name}`）+ `↓`（`:disabled="locked || index === albums.length - 1"`，aria-label `下移{name}`）+ `置顶`（link small，`:disabled="locked || index === 0"`，动作 `move(record.id, -albums.length)`）
    - `album`：`.admin-album-cell` = 72×54 封面 `<img>`（`:src="record.coverUrl"`，lazy，无封面时 `.admin-album-placeholder` + `tabler:album` 图标）+ 名称按钮（link，`:disabled="orderMode"`，点击 `navigateAlbum(record.id)`）+ `.admin-album-description`（`record.description || '暂无简介'`，单行省略号；桌面 max-width 420px，≤600px 为 200px，admin.css:31,60）
    - `actions`：`管理图片` → `navigateAlbum(record.id)`；`资料` → `navigateAlbum(record.id,'details')`；`下载` → `navigateAlbum(record.id,'downloads')`
- 顶部 `AAlert v-if="error"` type=error show-icon（albums:241）
- 列表视图**没有**格式 / 日期筛选与排序控件；只有「名称+简介」模糊搜索、20 条/页分页、行选择、以及手动排序模式。

### 1.2 工作区视图（`selectedId` 非空，albums:236-237、257-303）

- 返回条 `.admin-workspace-back`（`v-if="selectedId"`，albums:236）：`← 全部相册`（link，点击 `navigateAlbum()`，即 push `/dashboard/albums` 并清空 query）+ `/ {selectedAlbum?.name || '相册'}`（`.admin-help`）
- `DashboardPageHeader`（albums:237,239）
  - `title` = `selectedAlbum?.name || '加载相册'`
  - `description` = `{photoCount || 0} 张图片 · 在同一个工作区完成图片、资料和下载管理`
  - actions：
    1. `查看公开页面 ↗`：`<AButton :href="`/albums/${selectedId}`" target="_blank">`
    2. `刷新`：`:loading="detailLoading"`，`:disabled="locked"`，点击 `loadDetail()`
    3. `上传图片`：外层 `AUpload :accept="accept" multiple :show-upload-list="false" :before-upload="queueFile" :disabled="locked || !ready"`，内层 primary 按钮同 disabled（图标 `tabler:upload`）
- `AAlert v-if="detailError"` type=error show-icon（albums:258）
- `ASpin v-if="!ready && detailLoading"` `tip="加载相册…"`（albums:259）
- `AAlert v-if="!selectedAlbum && !loading && !detailLoading && !detailError"` type=warning `相册不存在，请返回列表重新选择。`（albums:260）
- `ATabs v-if="selectedAlbum && ready"`：`:active-key="tab"`，`@change="(key) => navigateAlbum(selectedId, String(key))"`（albums:261）

#### 页签 1：`photos`，label `图片管理（{photos.length}）`（albums:262-285）

- 队列提示：`AAlert v-if="albumUploads"` type=info：`{albumUploads} 张图片等待上传或处理中；可切换页面，成功入库后自动显示。` + `#action` 按钮 `查看队列`（`uploads.open.value = true`）（albums:263）
- `ACard` 内：
  1. **空相册态**（`!photos.length && !detailError`，albums:265）：大号 `AUploadDragger`（`:accept="accept"`、`multiple`、`:show-upload-list="false"`、`:before-upload="queueFile"`、`:disabled="locked"`）
     - 图标 `tabler:cloud-upload`
     - 主文案 `拖入图片，或点击开始上传`
     - 提示 `选择后自动上传，7 并发；入库时自动生成三层预览。`
  2. **有图片态**（albums:267-282）：
     - 紧凑 `AUploadDragger.admin-compact-upload`：`拖入更多图片，或点击选择 · 自动加入上传队列`（admin.css:33-34：块级，padding 12px，灰字）
     - 过滤工具条 `.admin-toolbar`（albums:268）
       - `AInputSearch photoQuery`：placeholder `搜索文件名`，aria-label `搜索图片`，allow-clear，`width:230px`
       - `ASelect photoFormat`：aria-label `筛选图片格式`，`width:130px`，选项 `全部格式/all`、`PNG/png`、`JPG / JPEG/jpg`、`WebP/webp`
       - `ASelect photoSort`：aria-label `图片排序`，`width:120px`，选项 `最近上传/newest`、`文件名称/name`、`文件大小/size`
       - 右组 `ARadioGroup photoView`：`option-type="button"`，aria-label `图片视图`，选项 `网格/grid`、`列表/table`
     - 选择条 `.admin-selection-bar`（albums:269；浅蓝底 `#f5faff` + `#d6e4ff` 边框，admin.css:46）
       - 左组：`ACheckbox` `本页全选`（`:checked="pageChecked"`、`:indeterminate="pagePartial"`、`:disabled="locked || !pagePhotos.length"`）；文本 `已选 {selectedPhotos.length} / {photos.length}`；link small `选择全部筛选结果（{filteredPhotos.length}）`（`:disabled="locked || !filteredPhotos.length"`）；link small `取消选择`（仅 `selectedPhotos.length` 非 0）
       - 右组（仅 `selectedPhotos.length` 非 0）：`设为封面`（`:disabled="locked || selectedPhotos.length !== 1"`）；`删除（{n}）`（danger，`:loading="mutation === 'delete-photos'"`，`:disabled="locked && mutation !== 'delete-photos'"` ← **注意这个怪异判定**）
     - **网格** `.admin-photo-grid`（albums:270-277；`repeat(auto-fill,minmax(160px,1fr))` gap 16px；≤600px 变 `repeat(2,1fr)` gap 10px，admin.css:32,60）
       - 每项 `<article class="admin-photo-tile" :class="{ selected: selectedPhotos.includes(photo.id) }">`（边框 2px；hover 蓝、选中 `#1677ff` + 浅蓝底，admin.css:35-37）
       - `AImage`：`src={/api/photos/{photo.id}/thumbnail?v=grid2}`、`preview={{ src: /api/photos/{photo.id}/preview }}`、`loading="lazy"`（4:3 裁切，admin.css:38-39）
       - 左上 `ACheckbox.admin-photo-check`：`:checked="selectedPhotos.includes(photo.id)"`、aria-label `选择图片：{originalName}`、`:disabled="locked"`、`@change="togglePhoto(photo.id)"`（**忽略事件参数，按当前是否已选取反**）
       - 右上 `ATag` `封面`（color=blue，`v-if="selectedAlbum.coverPhotoId === photo.id"`，`pointer-events:none`）
       - 底部 `<button class="admin-photo-caption" :title="photo.originalName" :disabled="locked" @click="togglePhoto(photo.id)">`：`{originalName}` + `{photo.format.toUpperCase()} · {adminBytes(photo.byteSize)}`
     - **列表** `ATable v-else-if="photoView === 'table'"`（albums:278-280）
       - `:data-source="pagePhotos"`（仅当前页）、`row-key="id"`、`size="middle"`、`:pagination="false"`、`:scroll="{ x: 700 }"`
       - `:row-selection="{ selectedRowKeys: selectedPhotos, onChange: keys => selectedPhotos = keys.map(String), preserveSelectedRowKeys: true, getCheckboxProps: () => ({ disabled: locked }) }"`
       - 列（albums:67）：`图片/key:'photo'/90`、`文件名/dataIndex:'originalName'`、`格式/dataIndex:'format'/90`、`尺寸/key:'dimensions'/140`、`大小/key:'size'/110`
       - 单元格：56×56 缩略图（`:width="56" :height="56" style="object-fit:cover"`，同上 `?v=grid2` + preview）、`{record.width} × {record.height}`、`adminBytes(record.byteSize)`
     - **空结果**：`AEmpty v-else description="没有符合条件的图片"`（albums:281）——该分支仅在「grid 视图且 `pagePhotos` 为空」时渲染；table 视图无数据走表格自带空态
     - **分页** `.admin-pagination`（albums:282；flex-wrap space-between，admin.css:47）
       - 左文本：`{filteredPhotos.length} 张符合条件 · 每页 48 张`
       - `APagination v-model:current="photoPage" :page-size="48" :total="filteredPhotos.length" :show-size-changer="false" show-quick-jumper`
       - `const photoPageSize = 48`（albums:45）

#### 页签 2：`details`，label `相册资料`（albums:286-301）

外层 `.admin-stack`（纵向 gap 24px，admin.css:13）。

1. `ACard title="名称、简介与展示日期"`
   - `AForm layout="vertical" :model="draft" :disabled="locked" @finish="save"`
   - `.admin-form-grid`（2 列，`column-gap:24px`；≤768px 变 1 列，admin.css:15,62）：
     - `相册名称`（required）：`AInput v-model:value="draft.name" :maxlength="100" show-count`
     - `展示创建日期`：`ADatePicker :value="draft.displayCreatedDate || undefined" value-format="YYYY-MM-DD" placeholder="自动日期"`，`extra="留空使用真实创建日期。"`，`@update:value` → `updateDate('displayCreatedDate', value)`
   - `相册简介`：`ATextarea v-model:value="draft.description" :rows="3" :maxlength="1000" show-count placeholder="显示在公开相册页面的介绍"`
   - 第二行 `.admin-form-grid`：`图片开始日期` / `图片结束日期`（同为 DatePicker，placeholder `自动日期`）
   - link 按钮 `恢复自动日期（保存后生效）`：把 `displayCreatedDate`、`photoDateStart`、`photoDateEnd` 三个都置 `null`
   - `AAlert v-if="formError"` type=error show-icon（放服务端/本地校验消息）
   - `.admin-save-bar`（**sticky bottom:12px**，z-index 5，白底 + 边框 + `0 4px 16px #00000008` 阴影；≤600px 变纵向，admin.css:48,60）
     - 左：`dirty ? '有未保存的修改' : '所有资料已保存'`（dirty 时 class `admin-unsaved` = `#ad6800`）
     - 右：`放弃修改`（`:disabled="!dirty || locked"` → `applyDraft(selectedAlbum)`）；`保存相册资料`（`html-type="submit"`，primary，`:disabled="!dirty"`，`:loading="mutation === 'save'"`）
2. `<DashboardAlbumCoverEditor :key="selectedId" :album="selectedAlbum" :photos="photos" :disabled="locked" @saved="applyCover" @busy="coverBusy = $event" />`（albums:298）—— `:key="selectedId"` 使切相册时组件整体重建（内部状态归零）
3. `ACard size="small"` 删除区（albums:299）
   - 标题 `删除相册`，说明 `将删除相册及其中全部图片、封面和压缩包，无法撤销。`
   - `删除此相册`（danger，`:disabled="locked || dirty || downloadDirty"`，`:loading="mutation === 'delete-album'"`）

#### 页签 3：`downloads`，label `公开下载`（albums:302）

`<DashboardDownloadManager :key="selectedId" :album-id="selectedId" embedded @dirty="downloadDirty = $event" @busy="downloadBusy = $event" />`
- `embedded` 模式下组件用 `props.albumId` 而非 route query（DownloadManager.vue:11）
- 内部每 3s 轮询 `/api/album-downloads`（`document.hidden` 时 15s），并在 `embedded` 时跳过自身路由守卫（DownloadManager.vue:142-152）
- `@dirty` → 本页 `downloadDirty`（参与 `离开确认` 与删除相册按钮禁用）；`@busy` → 本页 `downloadBusy`（参与 `locked`）

### 1.3 弹窗（与视图平级，albums:305-308）

1. **新建相册弹窗**：`AModal v-model:open="createOpen" title="新建相册" :footer="null" :closable="!locked" :mask-closable="!locked" :keyboard="!locked"`
   - `AForm layout="vertical" :model="newAlbum" :disabled="locked" @finish="create"`
   - `相册名称`（required）：`AInput :maxlength="100" placeholder="例如：2026 夏日旅行"`
   - `简介（选填）`：`ATextarea :rows="3" :maxlength="1000"`
   - `AAlert v-if="createError"` type=error
   - `.admin-dialog-actions`（右对齐 gap 8px，margin-top 20px，admin.css:50）：`取消`（`:disabled="locked"` → 关弹窗）／`创建并上传图片`（primary，submit，`:loading="mutation === 'create'"`）
2. **导出弹窗**：`AModal v-model:open="exportOpen" title="管理员导出原始文件" :footer="null"`
   - `导出选中的 {listState.selected.length} 个相册，保持入库文件格式，不改变公开下载设置。`
   - `.admin-help`：`一个相册生成一个 ZIP；多个相册生成一个外层 ZIP，其中每个相册各一个 ZIP。大相册需要等待服务器打包。`
   - 按钮：`取消`（关弹窗）／`下载原始文件`（primary，`:href="exportUrl"`，`:disabled="!listState.selected.length"`，`@click="exportOpen = false"`）

### 1.4 桌面端 vs 移动端差异

本页**没有任何移动端专属分支**，全部依赖响应式：

| 层 | 规则 | 位置 |
|---|---|---|
| 布局 | `useMediaQuery('(max-width: 991px)')`：≤991px 隐藏 224px `ALayoutSider`，改用 `ADrawer`(248px) 抽屉导航；顶栏保留面包屑 + `DashboardUploadQueue` + 退出按钮 | `app/layouts/dashboard.vue:10,72-80` |
| 页头 | ≤768px `.admin-page-header` 变纵向（标题在上，操作区左对齐）；`.admin-page-actions` 始终 flex-wrap | admin.css:12,62 |
| 内容 | ≤768px `.admin-content` padding 24px → 16px；`.admin-app .admin-topbar` padding 同上 | admin.css:62 |
| 表单 | `.admin-form-grid` 2 列 → ≤768px 1 列 | admin.css:15,62 |
| 图片网格 | `auto-fill minmax(160px,1fr)` → ≤600px `repeat(2,1fr)` gap 10px | admin.css:32,60 |
| 保存条 | ≤600px 变纵向（文字在上、按钮在下） | admin.css:60 |
| 相册简介 | 桌面 max-width 420px → ≤600px 200px | admin.css:31,60 |
| 工具条 | 全部 `ASpace wrap`；搜索框 `max-width:70vw` | albums:244,268 |
| 表格 | 统一 `:scroll="{ x: 700 }"` 横向滚动 | albums:249,278 |
| 封面组件 | 自身 `@media (max-width:640px)`：`.cover-editor` 纵向、预览 `max-width:320px`、选择网格 4 列 → 2 列 | COVER:126 |
| HERO | Tailwind `lg:`（1024px）断点 | HERO:14,26,30 |

---

## 2. 完整 API 调用清单

### 2.1 请求封装契约（API:102-126、199-212）

- `adminFetch(url, { method, body, query?, headers?, signal? })`
- 每次调用前：若 `!authState.checked || authState.loading` → 先 `GET /api/auth/status`；若 `!authState.authenticated` → **不发请求**，直接 `throw new Error('请先登录管理员账号')`（API:203-209）
- 请求头：
  - 始终 `X-Requested-With: ChronoFrame`（API:108）
  - 非 `GET`/`HEAD` 时，若 cookie `cf_csrf` 存在则加 `X-CSRF-Token: <cookie值>`（API:110-113；`readBrowserCookie` 用 `document.cookie` + `decodeURIComponent`）
  - `credentials: 'include'`（API:120）
- 响应 401 → 全局置为未登录（API:122-125）
- 后端对写操作强制 CSRF：cookie `cf_csrf` 与 header `x-csrf-token` 必须一致且匹配会话，否则 403 `CSRF 校验失败`（main.rs:2264-2275）
- 错误消息提取 `getAdminApiErrorMessage`：`error.data.error` → `error.data.message` → `error.message` → `'请求失败，请稍后重试'`（API:65-76）；`responseStatusOf` 依次读 `error.response.status` / `statusCode` / `status`（API:59-63）

### 2.2 相册管理页实际发出的全部调用

| # | 触发点 | Method | 完整路径 | 请求体 | 期望响应 | 代码位置 |
|---|---|---|---|---|---|---|
| 1 | 挂载 / 列表刷新按钮 / 保存顺序后（后者用其返回值） | GET | `/api/albums` | — | `Album[]`（200；后端排序 `position ASC, created_at DESC, id ASC`；**无鉴权要求**） | albums:103 |
| 2 | 进入相册 / 首挂载 / 详情刷新 / 删图后 / 上传成功后 1s | GET | `/api/albums/{encodeURIComponent(id)}` | — | `AlbumDetail` = 平铺的 `Album` 字段 + `photos: Photo[]`（photos 按 `created_at DESC, id DESC`；404 `相簿不存在`） | albums:87 |
| 3 | 创建相册 | POST | `/api/albums` | `{ name: string(trim), description: string(trim) }` | `Album`（**201 CREATED**；后端自增负 position 排到最前） | albums:147 |
| 4 | 保存资料 | PATCH | `/api/albums/{selectedId}`（**未 encodeURIComponent**） | `{ name: string(trim), description: string(trim), displayCreatedDate: string\|null, photoDateStart: string\|null, photoDateEnd: string\|null }` | `Album` | albums:160 |
| 5 | 保存顺序 | POST | `/api/albums/order` | `{ albumIds: string[] }`（必须=当前全部相册、无重复，否则 400 `相簿顺序必须完整包含当前所有相簿且不得重复`） | `Album[]`（后端复用 list_albums） | albums:227 |
| 6 | 删除相册 | DELETE | `/api/albums/{album.id}` | — | `AlbumDeletionResult`（详见 §2.3） | albums:205 |
| 7 | 图片页签「设为封面」 | PUT | `/api/albums/{selectedId}/cover` | `{ photoId: string }` | `AlbumCover` | albums:190 |
| 8 | COVER 从相册选图 | PUT | `/api/albums/{encodeURIComponent(albumId)}/cover` | `{ photoId: string }` | `AlbumCover` | COVER:37,60 |
| 9 | COVER 上传本地封面 | POST | `/api/albums/{encodeURIComponent(albumId)}/cover` | `FormData`，**唯一字段名 `file`**（`form.append('file', file)`），multipart；单次只允许一个文件 | `AlbumCover` | COVER:37,55-57 |
| 10 | COVER 恢复自动封面 | DELETE | `/api/albums/{encodeURIComponent(albumId)}/cover` | 无 body | `AlbumCover`（`coverSource` 变 `auto`） | COVER:37,79 |
| 11 | 批量删除图片 | POST | `/api/photos/delete` | `{ photoIds: string[] }`（后端去重；空集合 400） | `PhotoDeletionResult`（详见 §2.3） | albums:180 |
| 12 | 图片上传（队列，每文件 1 请求） | POST | `/api/albums/{encodeURIComponent(albumId)}/photos` | `FormData`，字段名 **`files`**，multipart，一次一个文件 | `Photo[]`（200；404 `相簿不存在；请先创建相簿`） | useAdminUploads.ts:13-14 |
| 13 | 导出原始文件 | GET（`<a href>` 浏览器导航，**非 fetch**） | `/api/albums/export?albumIds=a,b,c`（`new URLSearchParams({ albumIds: listState.selected.join(',') })`） | — | ZIP 流；错误：400（空/超 64 个/重复）、404（`相簿不存在：{id}`）、429（`当前打包任务较多，请稍后重试`） | albums:231,308 |
| 14 | 封面图（服务端返回的 URL，前端不自行拼接） | GET | `/api/albums/{albumId}/cover/{version}` | — | `image/webp`、`Cache-Control: private, max-age=31536000, immutable`、`X-Content-Type-Options: nosniff`；**version 不匹配立即 404** | album_covers.rs:257-281 |
| 15 | 缩略图 / 预览 | GET | `/api/photos/{id}/thumbnail?v=grid2`、`/api/photos/{id}/preview` | — | PNG 缩略图 / 预览图；`?v=grid2` 是前端固定的缓存 token，**后端不解析该参数**（main.rs:5552-5581） | albums:272,279；COVER:95 |

> 备注：`adminFetch` 支持 `query` 选项（API:6），但本页**没有一处使用**它；唯一带 query 的调用（导出）走的是 `<a href>`。

### 2.3 精确响应形状（TYPES:5-97；后端 main.rs:1485-1518、4649-4654、4742-4748）

```ts
interface AlbumCover {
  coverSource: 'auto' | 'photo' | 'upload'
  coverPhotoId: string | null
  coverUrl: string | null
}

interface Album extends AlbumCover {
  id: string
  name: string
  description: string
  createdAt: number            // Unix 秒
  displayCreatedDate: string | null   // 'YYYY-MM-DD'
  photoDateStart: string | null
  photoDateEnd: string | null
  position: number
  photoCount: number
}

interface Photo {
  id: string
  albumId: string
  originalName: string
  storageKey: string
  format: 'png' | 'jpg' | 'webp'   // 注意：jpeg 会被入库为 jpg
  contentType: string
  byteSize: number
  width: number
  height: number
  createdAt: number
}

interface AlbumDetail extends Album { photos: Photo[] }   // 后端 #[serde(flatten)]

interface PhotoDeletionResult {
  deleted: number
  objectsRemoved: number
  cleanupPending: number
  failures: { photoId: string; error: string }[]
}

interface AlbumDeletionResult {
  deleted: boolean
  photosDeleted: number
  objectsRemoved: number
  cleanupPending: number
  failures: { photoId: string; error: string }[]
}
```

**`coverUrl` 的生成规则**（album_covers.rs:51-84）——前端**必须直接使用服务端返回的 coverUrl**：

- `coverSource = 'photo'`（存在 `photo_id`）→ `/api/photos/{urlencode(photoId)}/thumbnail?v=grid2`
- `coverSource = 'upload'`（无 `photo_id` 但有 `version`）→ `/api/albums/{urlencode(albumId)}/cover/{urlencode(version)}`
- `coverSource = 'auto'`（两者皆无）→ 该相册最新一张图片的 thumbnail；无图时 `coverUrl = null`
- **version 语义**：每次 PUT/POST 封面都写入一个新的 UUID 作为 version（album_covers.rs:127-129），因此**旧 coverUrl 立即 404**，实现 immutable 长期缓存下的即时失效。React 重写时不要缓存或自行拼 URL，必须用响应里的 `coverUrl` 覆盖本地状态并在 `<img src>` 上使用它（可加 `?` 无参数，不要自行加 version）。

**图片上传的 multipart 细节**（main.rs:4751-4800）：后端遍历所有字段，取 `file_name()` 推断格式（png/jpg/jpeg/webp）；空文件跳过；单个请求内任一文件非法则整请求 400（`{filename} 仅支持 PNG、JPG/JPEG、WEBP`），所以前端必须**每文件一个请求**（`注释 main.rs:4781-4782` 明确说明这一设计）。

**上传队列契约**（shared/utils/admin-upload-queue.ts:27-73；useAdminUploads.ts）：

- 全局**单**队列，存于 `useState('admin-upload-queue')`；浮层开关 `useState('admin-upload-queue-open')`
- 并发 **7**，跨**所有**相册共享；配置 `concurrency = 7`
- item 形状：`{ id: number, albumId, albumName, name, size, file?: File, status: 'queued'|'uploading'|'done'|'failed', error: string }`
- 成功：`status='done'`、释放 `file` 引用、`state.albumVersions[albumId] = (state.albumVersions[albumId] || 0) + 1`
- **绝不中断已发出的上传**（丢失响应无法判断服务端是否已提交）
- 其他 API：`enqueue(files, {id, name})`、`pause()`、`resume()`、`retryFailed()`、`remove(id)`（uploading 中不可移除）、`clearDone()`

---

## 3. 全部用户交互与工作流

### 3.1 创建相册（albums:140-153、305-307）

1. 入口：列表页头 `新建相册` → `createOpen = true`
2. 表单字段：`相册名称`（required，maxlength 100，placeholder `例如：2026 夏日旅行`）、`简介（选填）`（textarea rows 3，maxlength 1000）；整体受 `locked` 禁用
3. 提交（`@finish="create"`，按钮 `创建并上传图片`，`:loading="mutation === 'create'"`）：
   - 守卫：`if (locked) return`
   - **本地校验**：`validateAlbumDraft({ ...draft, ...newAlbum, displayCreatedDate: null, photoDateStart: null, photoDateEnd: null })` —— 以**资料编辑草稿**为底，再用新建表单的 `name`/`description` 覆盖，三个日期强制为 `null`；返回非 null 则写入 `createError` 并 return（不发请求）
   - `mutation = 'create'` → `POST /api/albums` body `{ name: trim, description: trim }`
   - 成功：`applyAlbum(created)`（按 id 存在则替换，否则 push 到 `albums` 末尾）→ `createOpen = false` → 清空 `newAlbum.name/description` → 清 `createError` → 通知 `相册已创建，可以上传图片了`（success）
   - 失败：`createError = getAdminApiErrorMessage(cause)`，弹窗保持打开
   - `finally`：`mutation = ''`
   - **`if (created) await navigateAlbum(created.id)`**（在 finally 之后）：直接进入新相册的**图片管理页签**（query 只有 `album`）
4. 取消：`取消` 按钮置 `createOpen = false`（`locked` 时禁用；弹窗不可点遮罩/ESC 关闭）

### 3.2 编辑相册资料（albums:154-165、286-297）

- **统一保存**（点击底部 sticky 保存条），**没有即时保存**。
- 草稿与脏检查：
  - `draft = reactive<AlbumDraft>({ name:'', description:'', displayCreatedDate:null, photoDateStart:null, photoDateEnd:null })`（albums:29）
  - `applyDraft(album)`：`Object.assign(draft, albumDraftOf(album))` → `baseline = JSON.stringify(draft)` → `formError = ''`（albums:71）
  - `albumDraftOf`（shared/utils/admin-albums.ts:9-15）：`description: album.description || ''`，三个日期 `x || null`
  - `dirty = !!baseline && JSON.stringify(draft) !== baseline`（albums:31）→ **基于 JSON 字符串比较，key 顺序敏感**；`baseline=''`（未加载）时永远不脏
- `updateDate(field, value)`：`draft[field] = (typeof value === 'string' && value) ? value : null`（albums:72）→ DatePicker 清空即 null
- 保存（`save()`，albums:154-165）：
  1. `if (locked || !ready || !dirty) return`
  2. `formError = validateAlbumDraft(draft) || ''`；非空则 return（**本地校验先于请求**）
  3. `mutation = 'save'` → `PATCH /api/albums/{selectedId}`，body `{ ...draft, name: draft.name.trim(), description: draft.description.trim() }`
  4. 成功：`applyAlbum(updated)` + `applyDraft(updated)`（dirty 归零）+ 通知 `相册资料已保存`（success）
  5. 失败：`formError = getAdminApiErrorMessage(cause)`（表单下方红色 Alert）
  6. `finally mutation = ''`
- `放弃修改`：`applyDraft(selectedAlbum)` —— 从当前相册对象重算草稿、重置 baseline、清错误（`:disabled="!dirty || locked"`）
- `恢复自动日期（保存后生效）`：三个日期字段置 null（仍需点保存才生效）
- 详情加载后**只在 `!dirty` 时**覆盖草稿：`if (!dirty.value) applyDraft(album)`（albums:92）→ 刷新不会打断正在编辑的内容
- 校验规则（shared/utils/admin-albums.ts:17-23，**逐字保留消息**）
  - `名称需为 1–100 个字符`（trim 后按 `Array.from` 计 code point）
  - `简介不能超过 1000 个字符`
  - `请同时填写图片起止日期，或同时留空使用自动日期`（`!!start !== !!end`）
  - `图片开始日期不能晚于结束日期`（字符串比较 `start > end`）
- 后端同规则校验（main.rs:3912-3926）并额外要求 PATCH **至少包含一个字段**（否则 400 `至少需要提供一个相簿字段`）；起止日期一个给一个不给 → 400 `图片开始日期和结束日期必须同时设置或同时清除`（main.rs:4113-4117）

### 3.3 设置封面（auto / photo / upload 三来源）

#### A. 图片管理页签的快捷入口（albums:186-195）

- 选择条右侧 `设为封面`，守卫 `if (locked || selectedPhotos.length !== 1) return`（**必须恰好选中 1 张**）
- `mutation = 'cover'` → `PUT /api/albums/{selectedId}/cover` body `{ photoId: selectedPhotos[0] }`
- 成功：`applyCover(selectedId, cover)`（`Object.assign(album, cover)`）→ 通知 `已设为相册封面`（success）
- 失败：通知 `封面设置失败` + 原因（error）
- `finally mutation = ''`

#### B. COVER 组件（资料页签，COVER 全文）

- props：`{ album: Album; photos: Photo[]; disabled: boolean }`；emits：`saved:[id, cover]`、`busy:[boolean]`（COVER:5-6）
- Card `title="相册封面"`，右上 `ATag` 展示来源：`coverSource==='upload'`→`单独上传`；`'photo'`→`相册选图`；否则`自动封面`；color：`auto`→`default`，其余 `blue`（COVER:19,65）
- 预览区 `.cover-preview`：240px 宽、`aspect-ratio:4/3`、`object-fit:cover`、圆角 8px、灰底；`:aria-busy="busy"`；无封面时 `AEmpty :image="AEmpty.PRESENTED_IMAGE_SIMPLE" description="暂无封面"`（COVER:67-70,110-111）
- 三个操作按钮（COVER:74-82）
  1. **`从相册选择`**（primary，`:disabled="disabled || !photos.length"`）→ `openPicker()`（COVER:22-29）：`selectedId = album.coverPhotoId`、`query=''`、`page=1`、`error=''`，`pickerOpen=true`
     - 弹窗：`AModal :width="800" title="从相册选择封面" ok-text="设为封面" cancel-text="取消" :confirm-loading="busy" :ok-button-props="{ disabled: !selectedPhoto || busy }" :cancel-button-props="{ disabled: busy }" :closable="!busy" :keyboard="!busy" :mask-closable="!busy" @ok="confirmSelection"`
     - `confirmSelection()` → `if (selectedPhoto) save('PUT', { photoId: selectedPhoto.id })`（COVER:60）
     - 内容：`AInputSearch`（placeholder `搜索图片文件名`，aria-label `搜索封面图片`，allow-clear，`:disabled="busy"`）→ error `AAlert` → `.cover-picker` 网格（`repeat(4,minmax(0,1fr))`，`max-height:52vh` 可滚动，≤640px 变 2 列）
     - 每个候选：`<button class="cover-choice" :class="{ chosen: selectedId === photo.id }" :aria-pressed :aria-label="'选择封面：' + photo.originalName" :disabled="busy" @click="selectedId = photo.id">`，内含：`<img :src="/api/photos/{encodeURIComponent(id)}/thumbnail?v=grid2" loading="lazy" draggable="false">` + `.cover-filename`（`title=originalName`，单行省略）+ 选中时右上 `✓` 圆标（`.cover-check`，蓝底白字 24px）
     - 空态：`AEmpty description="没有匹配的图片"`
     - footer `.cover-picker-footer`：左 `已选择：{name}` 或 `请选择一张图片作为封面`；右 `APagination v-model:current="page" :total="filtered.length" :page-size="24" :show-size-changer="false" size="small" :disabled="busy" hide-on-single-page`
     - 本地过滤：`photos.filter(p => p.originalName.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))`；`pageSize = 24`；`watch(query)` → `page = 1`（COVER:16-20）
  2. **`从电脑上传`**（`AUpload accept=".png,.jpg,.jpeg,.webp" :show-upload-list="false" :multiple="false" :before-upload="upload" :disabled="disabled"`；按钮 `:loading="busy"`、`:disabled="disabled && !busy"`）
     - `upload(file)`（COVER:50-59）：正则 `/\.(png|jpe?g|webp)$/i` 不通过 → `error='请选择 PNG、JPG/JPEG 或 WebP 图片'` 并 `return false`（**不发请求**）；通过 → `new FormData()` + `form.append('file', file)` + `void save('POST', form)`，**始终 `return false`** 阻止 antd 自身上传
  3. **`恢复自动封面`**（`APopconfirm title="恢复自动封面？" description="只移除手动封面设置，不会删除相册里的照片。" ok-text="恢复" cancel-text="取消" :disabled="disabled || album.coverSource === 'auto'" @confirm="save('DELETE')"`）
- 统一 `save(method, body?)`（COVER:30-49）
  1. 守卫 `if (disabled || busy) return`
  2. `busy = true`；`emit('busy', true)`；`error = ''`
  3. `adminFetch<AlbumCover>(`/api/albums/${encodeURIComponent(albumId)}/cover`, { method, body })`
  4. 成功：`emit('saved', albumId, cover)` → `clearNuxtData(['public-albums', 'album-detail'])`（失效公开页缓存，对应 `app/pages/albums/[id].vue:10` 的 `useAsyncData('album-detail')`）→ `pickerOpen = false` → 通知 `method === 'DELETE' ? '已恢复自动封面' : '相册封面已更新'`（success）
  5. 失败：`error = getAdminApiErrorMessage(cause)`；`!pickerOpen` 时卡片下方显示 `AAlert type=error`；同时通知 `封面保存失败，请刷新确认后重试` + 原因（error）
  6. `finally`：`busy = false`；`emit('busy', false)`
- 父级接线：`@busy="coverBusy = $event"` → `locked` 变 true → **整页冻结**（albums:28,298）；`@saved="applyCover"` → `Object.assign(album, cover)`（albums:78-81）
- 文案：`为相册选一张封面`、`用于相册首页卡片和详情页背景。选择后立即保存，不改变相册内图片的顺序。`、`支持 PNG、JPG/JPEG、WebP。单独上传的封面不计入照片数量，也不包含在下载包中。`、busy 时 `role="status"` 的 `正在上传并保存封面，请稍候…`（COVER:72-73,83-84）
- 服务端封面处理契约：单独上传的封面会**转码为 WebP**，最长边 800px、最大 200KB（超限压缩/缩图），扩展名必须与实际格式一致，否则 `封面扩展名与实际图片格式不一致`（album_covers.rs:6-7,168-186）；选图封面必须属于该相册，否则 400 `请选择当前相册内的图片；图片可能已经被删除或移动`（album_covers.rs:106-119）

#### C. 三来源汇总表

| `coverSource` | 产生方式 | 前端请求 | coverUrl 形态 |
|---|---|---|---|
| `auto` | 初始状态；或 DELETE 恢复 | `DELETE /api/albums/{id}/cover` | 最新图片的 `thumbnail?v=grid2`；无图则 `null` |
| `photo` | 图片页签「设为封面」、COVER「从相册选择」 | `PUT` + `{ photoId }` | `/api/photos/{id}/thumbnail?v=grid2` |
| `upload` | COVER「从电脑上传」 | `POST` + `FormData(file)` | `/api/albums/{id}/cover/{新UUID}` |

### 3.4 相册排序模式（albums:217-230、244-247、251）

- **进入**：列表页头 `调整顺序`（`:disabled="loading || locked || orderMode || albums.length < 2"`）→ `startOrder()`：`orderIds = albums.map(item => item.id)`（快照当前顺序）、`orderMode = true`（albums:217）
- **进入后的 UI 变化**：搜索框与「{n} 个相册」消失；左组换成 `调整首页展示顺序` + `上移、下移或置顶，最后统一保存。`；表格列变 `顺序 | 相册 | 图片`（无「操作」列）；分页关闭；行选择移除；「相册」列的名称按钮禁用（`:disabled="orderMode"`）
- **上移 / 下移 / 置顶**（albums:218-223、251）：
  - `move(id, delta)`：`index = orderIds.indexOf(id)`；`target = Math.max(0, Math.min(orderIds.length - 1, index + delta))`；`index < 0 || index === target` 直接 return；否则 `splice(index,1)` 后 `splice(target,0,id)`
  - `↑` = `move(id, -1)`；`↓` = `move(id, 1)`；`置顶` = `move(id, -albums.length)`（负数很大，被 clamp 到 0）
  - 禁用：首行 `↑`/`置顶`，末行 `↓`（末行判断用 `albums.length - 1`）；`locked` 时全部禁用
- **脏检查**：`orderDirty = orderMode && orderIds.join() !== albums.value.map(i => i.id).join()`（albums:39）→ `保存顺序` 按钮 `:disabled="!orderDirty"`
- **统一保存**：`saveOrder()` → `mutation='order'` → `POST /api/albums/order { albumIds: orderIds }` → 成功：`albums = 响应`（服务端权威顺序）、`orderMode = false`、通知 `相册顺序已保存`（success）；失败：通知 `顺序保存失败，请刷新后重试` + 原因（error），**保持 orderMode 以便重试**；`finally mutation=''`
- **取消恢复**：`取消` 按钮仅 `orderMode = false`（若 `locked` 禁用）；`orderIds` 保留但不再参与渲染；`filteredAlbums` 回到 `albums` 原顺序（albums:49-51）→ 天然“取消即恢复”
- 排序模式下的过滤：`filteredAlbums` 直接返回 `orderIds.map(id => albums.find(...)).filter(Boolean)`，**完全跳过搜索过滤**（albums:49-51）
- `orderDirty` 参与离开守卫与 beforeunload（albums:112,117）

### 3.5 删除相册（albums:196-216、299）

1. 入口：资料页签底部 danger `删除此相册`，`:disabled="locked || dirty || downloadDirty"`，`:loading="mutation === 'delete-album'"`
2. **前置守卫（前端）**：`if (!selectedAlbum || locked) return`；若 `albumUploads > 0`（该相册队列中 `queued`/`uploading` 的条数，albums:61）→ `uploads.open = true` + 通知 `请先完成或取消此相册的上传队列`（warning）+ return
3. **二次确认**：`mutation = 'confirm-delete-album'` → `notice.confirm('永久删除相册「{album.name}」及其中全部 {album.photoCount} 张图片？对应存储文件、封面和本地 ZIP 也会清理，无法撤销。', true)`；取消 → `mutation = ''` + return（`true` = ok 按钮 danger）
4. **执行**：`mutation = 'delete-album'` → `DELETE /api/albums/{album.id}`
5. **成功（`result.deleted === true`）**：`baseline = ''`、`downloadDirty = false`、`albums = albums.filter(a => a.id !== album.id)`、`listState.selected = listState.selected.filter(id => id !== album.id)`、通知 `已删除「{name}」`（`cleanupPending` 时 description `剩余存储文件将在后台继续清理`，color `warning`，否则 success）；`deleted === false` 时**不导航也不通知**
6. **导航**：`if (deleted) await navigateAlbum()` → push `/dashboard/albums`（清空 query，回到列表）
7. **失败**：通知 `删除失败，请刷新确认` + 服务端原因（error），停留在工作区
8. **级联与占用拒绝（后端，必须原样展示消息）**（main.rs:4675-4748、4552-4585）
   - `deleted: true`、`photosDeleted` = 该相册照片数、`objectsRemoved` = 本轮实际删除的存储对象数、`cleanupPending` = 失败待清理数、`failures[{photoId,error}]`
   - 删除相册会**逐张照片预检** `ensure_photo_deletable`：任一照片不可删 → 整个相册删除失败（409），提示为下列之一：
     - `所选图片正被转换任务或旧图清理任务使用，请先结束相关任务`（conversion_items 中 queued/processing 引用该图，或 source_deletion_outbox 有待清理项）
     - `所选图片属于尚未收尾的存储迁移，请先继续迁移并处理旧存储`
   - 其他 409/404：`存储正在迁移或清理，请稍后再删除相簿`（storage_mutation_gate 被占）、`相簿已由其他请求删除，请刷新页面`（并发删除）、404 `相簿不存在`
   - 成功后服务端会把剩余相册的 position 重新压紧为连续序号（main.rs:4734-4737）

### 3.6 图片管理（albums:166-185、262-285）

- **上传**
  - 三个入口：页头 `上传图片`、空相册大拖拽区、有图后的紧凑拖拽区
  - 全部走 `queueFile(file)`（albums:166-171）：
    - `if (!selectedAlbum || locked || !ready) return false`
    - 文件名不匹配 `/\.(png|jpe?g|jepg|webp)$/i` → 通知 `不支持此文件：${file.name}`（warning）
    - 否则 `uploads.enqueue([file], { id, name })`
    - **始终 `return false`**
  - `accept = '.png,.jpg,.jpeg,.jepg,.webp'`（albums:47，**`jepg` 拼写照抄**）
  - 上传成功 → `albumVersions[albumId]++` → 本页 1 秒去抖后 `loadDetail()`（albums:132-136）
- **网格/列表切换**：`ARadioGroup photoView`（button 样式，`grid`/`table`），状态 `useState('admin-photo-view')` → 跨相册切换、跨路由导航保留（albums:43）
- **搜索**：`photoQuery`，匹配 `originalName` 小写包含（trim 后）（albums:55）
- **格式筛选**：`photoFormat`（`all|png|jpg|webp`），与 `photo.format` **全等**（albums:55；注意入库时 jpeg → `jpg`，options 里的 `JPG / JPEG` 合并为值 `jpg`）
- **排序**（albums:56）
  - `newest`（默认）：`b.createdAt - a.createdAt || b.id.localeCompare(a.id)`
  - `name`：`a.originalName.localeCompare(b.originalName)`
  - `size`：`b.byteSize - a.byteSize`（大→小）
- **分页**：纯客户端分页，`photoPageSize = 48`，`pagePhotos = filteredPhotos.slice((page-1)*48, page*48)`（albums:45,58）
- **本页全选**：`ACheckbox 本页全选`，`:checked="pageChecked"`（本页非空且全部已选），`:indeterminate="pagePartial"`（非全选但本页有选中）；`@change` → `selectedPhotos = toggleVisibleSelection(selectedPhotos, pagePhotos.map(p => p.id), event.target.checked)`（albums:59-60,269）
- **选择全部筛选结果**：`selectedPhotos = toggleVisibleSelection(selectedPhotos, filteredPhotos.map(p => p.id), true)`，标签 `选择全部筛选结果（{filteredPhotos.length}）`，`:disabled="locked || !filteredPhotos.length"`（albums:269）——作用范围是**当前筛选后的全量**，不是当前页
- **取消选择**：`selectedPhotos = []`（albums:269）
- **跨页保留选择**：见 §4
- **设封面**：见 §3.3A
- **批量删除**（albums:173-185）
  1. `if (locked || !selectedPhotos.length) return`
  2. `ids = [...selectedPhotos]`（**先拷贝快照**，避免请求期间选择变化）
  3. `mutation = 'confirm-delete-photos'` → `notice.confirm('永久删除选中的 {ids.length} 张图片？本地、S3 或 WebDAV 中的对应图片和缓存也会删除，无法撤销。', true)`；取消 → `mutation = ''` + return
  4. `mutation = 'delete-photos'` → `POST /api/photos/delete { photoIds: ids }`
  5. **`await loadDetail()`** 重新拉取整个详情（照片列表与相册 `photoCount` 以服务端为准）
  6. 通知 `已删除 {result.deleted} 张图片`，`cleanupPending` 时 description = `{cleanupPending} 个存储对象将在后台继续清理`，color = `(result.failures.length || result.cleanupPending) ? 'warning' : 'success'`
  7. 失败：通知 `删除失败，请刷新确认` + 原因（error）
  8. `finally mutation = ''`
  - 图片级删除同样受 §3.5 的三类 409 占用限制（后端共用 `ensure_photo_deletable`）

### 3.7 导出原始文件（批量 ZIP）（albums:231、246、308）

1. 入口：列表工具条 `导出原始文件（{n}）`，仅当 `listState.selected.length > 0` 时渲染（`n` = 已选相册数）
2. 弹窗 `AModal title="管理员导出原始文件" :footer="null"`（无遮罩关闭限制，纯展示 + 跳转）
   - 正文 `导出选中的 {n} 个相册，保持入库文件格式，不改变公开下载设置。`
   - 帮助 `一个相册生成一个 ZIP；多个相册生成一个外层 ZIP，其中每个相册各一个 ZIP。大相册需要等待服务器打包。`
   - `取消` → 关弹窗；`下载原始文件`（primary，`:href="exportUrl"`，`:disabled="!listState.selected.length"`，`@click="exportOpen = false"`）
3. `exportUrl = `/api/albums/export?${new URLSearchParams({ albumIds: listState.selected.join(',') })}``（albums:231）
   - **浏览器直接导航下载，不经 fetch**；因此无需 CSRF 头（该接口后端只校验会话，`require_admin(..., false)`，main.rs:4349）
   - 服务端限制与行为：`albumIds` 为空 → 400 `请至少选择一个相簿`；>64 → 400 `一次最多打包 64 个相簿`；重复 → 400 `相簿选择中存在重复项`；相册不存在 → 404 `相簿不存在：{id}`；打包槽位 5s 拿不到 → 429 `当前打包任务较多，请稍后重试`（main.rs:4350-4402）
   - 单相册 → 一个 ZIP；多相册 → 外层 ZIP（Stored 压缩）内嵌每相册一个 ZIP（Deflated），文件名做去重与非法字符清理（main.rs:4266-4342）
4. 相册内的「公开下载」页签是另一套（DownloadManager，非本页导出）：相关接口 `GET /api/album-downloads`、`PUT /api/albums/{id}/download-settings { enabled, formats, maxImageBytes, maxZipBytes }`、`POST /api/albums/{id}/downloads/rebuild`、`POST /api/album-downloads/{id}/cancel`、`DELETE /api/album-downloads/{id}`、`PUT /api/album-downloads/settings/bulk { target, settings }`、下载链接 `/api/albums/{albumId}/downloads/{format}?version={jobId}`（DownloadManager.vue:62,80,114,126,136,199）

### 3.8 离开守卫与未保存保护（albums:110-118）

- `locked = !!mutation || coverBusy || downloadBusy`（albums:28）
- `leave()`（albums:110-113）：
  1. `if (locked)` → 通知 `正在提交操作，请稍候`（warning）→ `return false`（阻止导航）
  2. 若 `!dirty && !downloadDirty && !orderDirty` → `return true`
  3. 否则 `return await notice.confirm('有未保存的修改，确定放弃修改并离开吗？')`
- `onBeforeRouteLeave(leave)`（albums:114）
- `onBeforeRouteUpdate((to, from) => to.query.album === from.query.album ? true : leave())`（albums:115）——**同相册内切页签不弹确认，跨相册切换会弹**
- `useEventListener('beforeunload', e => { if (dirty || downloadDirty || orderDirty || locked) { e.preventDefault(); e.returnValue = '' } })`（albums:116-118）

---

## 4. 选择状态管理

### 4.1 图片选择 `selectedPhotos: ref<string[]>`（albums:46）

- **key = `photo.id`（string）**；数组顺序 = 添加顺序（`toggleVisibleSelection` 用 `Set` 迭代保序）
- 核心工具（shared/utils/admin-albums.ts:25-28，**逐字移植**）：
  ```ts
  export function toggleVisibleSelection(selected: string[], visible: string[], checked: boolean): string[] {
    const ids = new Set(selected)
    for (const id of visible) { if (checked) ids.add(id); else ids.delete(id) }
    return [...ids]
  }
  ```
  语义：对「可见集合」做并集/差集，**不触碰可见集合之外的 id** —— 这就是跨页保留选择的基础机制。
- **跨页保留**（同一会话内）：
  - 网格视图：全选/取消只把**当前页 48 条** (`pagePhotos`) 作为 visible 传入，其它页已选 id 原样保留；翻页、切换网格/列表都不影响 `selectedPhotos`
  - 列表视图：`row-selection` 带 `preserveSelectedRowKeys: true`，`onChange(keys)` 直接赋值 `selectedPhotos = keys.map(String)`（antd 会把非当前页的已选 key 一并回传）
  - 单个切换（网格）：`togglePhoto(id)` → `toggleVisibleSelection(selectedPhotos, [id], !selectedPhotos.includes(id))`（albums:172）；勾选框的 change 事件参数被忽略
- **与「选择全部筛选结果」的关系**：该按钮把 `filteredPhotos`（当前搜索 + 格式筛选后的**全量**，可含未在当前页的照片）全部并入选中（`checked = true`），是**只增不减**的并集操作；再次点击（筛选条件变化后）继续并入新集合；要清空只能点「取消选择」或逐张取消
- **清空 / 收缩选择的时机**
  1. `watch(selectedId)`（切相册）：`selectedPhotos = []`（albums:122）——切相册立即清空
  2. `loadDetail` 成功后：`selectedPhotos = selectedPhotos.filter(photoId => loadedPhotos.some(p => p.id === photoId))`（albums:93）——`loadedPhotos` 是该相册**全部**照片，所以同相册内翻页/刷新/上传后选择依然保留，只有「已不存在的照片 id」被剔除；**批量删除后正是靠这一步收紧选择**
  3. 显式「取消选择」→ `[]`
  4. 组件卸载不持久化（无 localStorage）
- 派生状态（albums:59-60）：
  - `pageChecked = pagePhotos.length > 0 && pagePhotos.every(p => selectedPhotos.includes(p.id))`
  - `pagePartial = !pageChecked && pagePhotos.some(p => selectedPhotos.includes(p.id))`
- 计数展示：`已选 {selectedPhotos.length} / {photos.length}` —— **分母是相册全部图片数**，不是筛选结果数（albums:269）
- 注意：`设为封面` 要求 `selectedPhotos.length === 1`（跨页多选会因 >1 而禁用）

### 4.2 相册选择 `listState.selected: string[]`（albums:18）

```ts
const listState = useState('admin-album-list-view', () => ({ query: '', page: 1, selected: [] as string[] }))
```

- **key = `album.id`（string）**；`onChange: keys => listState.selected = keys.map(String)` + `preserveSelectedRowKeys: true`（跨页保留）
- `useState` 是 Nuxt 应用内存态：**跨路由导航保留**（进入相册工作区再返回，搜索词 / 页码 / 选择仍在），**整页刷新即丢失**（`ssr: false`，无 localStorage 持久化）→ React 重写需要一个模块级 / Context 级 store 复刻这一语义
- 同一 store 还带 `query`（搜索词）与 `page`（页码，pageSize 20）
- 收缩时机：
  - `loadAlbums` 成功后：`listState.selected = listState.selected.filter(id => albums.some(a => a.id === id))`（albums:104）
  - 删除相册后：`listState.selected = listState.selected.filter(id => id !== album.id)`（albums:210）
  - 「取消选择」按钮 → `[]`
- 用途：导出原始文件的目标集合、「导出原始文件（{n}）」与「取消选择」按钮的显隐
- 关联的共享状态：`photoView` = `useState('admin-photo-view', () => 'grid')`（albums:43）；上传队列 `useState('admin-upload-queue')` 与 `useState('admin-upload-queue-open')`（useAdminUploads.ts:7-8）
- 页面局部（**不**共享、切相册即重置）：`photoQuery`、`photoFormat`、`photoPage`（albums:125）；**`photoSort` 不重置**（相册切换后保留当前排序）

---

## 5. 地址栏 / 页签深链（必须逐字保真）

### 5.1 唯一真源 = route.query

```ts
const selectedId = computed(() => typeof route.query.album === 'string' ? route.query.album : '')          // albums:15
const tab = computed(() => ['details', 'downloads'].includes(String(route.query.tab)) ? String(route.query.tab) : 'photos')  // albums:17
const selectedAlbum = computed(() => albums.value.find(item => item.id === selectedId.value))              // albums:16
```

- **query 参数名**：`album`、`tab`（全部小写，无连字符）
- `album` 非 string（缺失 / 数组 / 重复参数）→ 视为 `''` → 渲染列表视图
- **`tab` 的取值域只有 `details` 和 `downloads` 被承认；其余一切值（包括缺失）都回落 `'photos'`**
- **默认页签 `photos` 不写入 URL**；非法 `tab` 值只在内存里回落，**不会**被规范化/清除掉 URL 里的脏值（保真重写时不要“顺手清洗”）

### 5.2 唯一的写入函数（albums:109）

```ts
const navigateAlbum = (id = '', nextTab = 'photos') =>
  router.push({
    path: '/dashboard/albums',
    query: id ? { album: id, ...(nextTab === 'photos' ? {} : { tab: nextTab }) } : {},
  })
```

- 使用 **`router.push`（不是 replace）** → 每次进入相册 / 切页签都产生历史记录，浏览器后退可用
- `id` 为空 → `query: {}` → 纯 `/dashboard/albums`（列表视图）
- `nextTab === 'photos'` → 只写 `album`；`nextTab === 'details' | 'downloads'` → 写 `album` + `tab`

### 5.3 精确 URL 形态

| 场景 | URL | 触发点 |
|---|---|---|
| 列表 | `/dashboard/albums` | 首屏无 query；`← 全部相册`；相册删除成功后 |
| 图片管理（默认页签） | `/dashboard/albums?album=<id>` | 点相册名 / `管理图片`；新建相册成功后；`navigateAlbum(id)` 或 `navigateAlbum(id,'photos')` |
| 资料页签 | `/dashboard/albums?album=<id>&tab=details` | 列表 `资料` 按钮；页签点击 `details` |
| 公开下载页签 | `/dashboard/albums?album=<id>&tab=downloads` | 列表 `下载` 按钮；页签点击 `downloads` |

### 5.4 页签切换与 watch 逻辑

- **页签组件本身没有本地 state**：`<ATabs :active-key="tab" @change="key => navigateAlbum(selectedId, String(key))">`（albums:261）→ 点击页签就是改 URL；因为 `onBeforeRouteUpdate` 只在 `to.query.album !== from.query.album` 时才调用 `leave()`（albums:115），同相册切页签**不触发未保存确认**
- **没有 watch(route/tab) 做双向同步**：`tab` 与 `selectedId` 都是纯 computed（albums:15-17），URL 变化自动驱动渲染；React 重写应保持这种单向数据流（`useSearchParams()` 派生，不要另建 state，否则会出现双写不一致）
- **有 watch(selectedId)**（albums:119-127）：切相册时清空 `uploadRefresh` 定时器、`baseline=''`、`ready=false`、`photos=[]`、`selectedPhotos=[]`、`detailError=''`、`orderMode=false`、`orderIds=[]`、`downloadDirty=false`、`downloadBusy=false`、`coverBusy=false`、`photoQuery=''`、`photoFormat='all'`、`photoPage=1`，然后 `loadDetail(id)`
- **首屏深链**：`onMounted(() => { void loadAlbums(); void loadDetail() })`（albums:137），`loadDetail(id = selectedId.value)`；`if (!id) { detailLoading = false; return }`（albums:82-84）→ `?album=X&tab=downloads` 直接刷新也能落到下载页签（Tabs 渲染条件是 `selectedAlbum && ready`，albums:261）
- **不存在相册**：详情 404 → `detailError` 显示错误 Alert；若只是本地列表里找不到（数据不一致）→ 显示 `相册不存在，请返回列表重新选择。`（albums:260）
- 另一处 query 使用者：独立页 `/dashboard/downloads` 用 `route.query.album`（DownloadManager.vue:11,72,175）；本页是 `embedded` 模式，用 `album-id` props，不读 query
- 导航到不同相册时有未保存内容 → `leave()` 弹确认；用户取消 → 路由不变

---

## 6. 加载与错误处理

### 6.1 状态量（albums:19-37、68-70）

| 状态 | 初值 | 含义 |
|---|---|---|
| `loading` | false | 相册列表请求中（表格 loading + `刷新` 按钮 loading） |
| `detailLoading` | false | 详情请求中（页头 `刷新` loading；`!ready && detailLoading` 时显示 Spin） |
| `ready` | false | 详情至少成功加载过一次（`ATabs` 与保存/上传的渲染前提） |
| `error` | '' | 列表错误（顶部 Alert） |
| `detailError` | '' | 详情错误（顶部 Alert） |
| `mutation` | '' | 单值令牌：`'' \| 'create' \| 'save' \| 'cover' \| 'confirm-delete-photos' \| 'delete-photos' \| 'confirm-delete-album' \| 'delete-album' \| 'order'` |
| `coverBusy` / `downloadBusy` | false | 由 COVER / DownloadManager 通过 `busy` 事件上抛 |
| `downloadDirty` | false | 由 DownloadManager 通过 `dirty` 事件上抛 |
| `locked` | computed | `!!mutation \|\| coverBusy \|\| downloadBusy` → 冻结搜索、刷新、上传、表单、表格选择、封面、删除、排序等所有控件 |
| `detailSerial` / `disposed` / `uploadRefresh` | 0 / false / undefined | 竞态与去抖用的模块级变量（非响应式） |

### 6.2 并发去重与竞态处理

1. **相册列表**：`loadAlbums` 首行 `if (loading.value) return`（albums:100）→ 并发调用被**丢弃**（不是排队、不合并）
2. **详情串行号**（albums:82-98）：
   ```ts
   const serial = ++detailSerial
   ... await adminFetch<AlbumDetail>(...)
   if (disposed || serial !== detailSerial || selectedId.value !== id) return
   ```
   → 迟到的旧响应被整体丢弃；`detailError` / `detailLoading` 只在 `serial === detailSerial` 时写入；`onBeforeUnmount` 里 `disposed = true; detailSerial++`（albums:138）
3. **上传完成后的刷新去抖**（albums:132-136）：`watch(() => uploads.state.value.albumVersions[selectedId.value], ...)`，用 `setTimeout(..., 1000)`，并有 `if (uploadRefresh) return` 防止连续上传不断推迟；切相册时 `clearTimeout(uploadRefresh)`（albums:120-121）
4. **鉴权状态去重**：模块级 `pendingAuthStatusRequest` 保证并发请求只发一次 `/api/auth/status`（API:39,128-152）
5. **DownloadManager**：`loadRequest` 串行去重 + `poll()` 3s 轮询（`document.hidden` 时 15s），卸载时 `clearTimeout`（DownloadManager.vue:49-70,142-144）
6. **删除图片/相册前的 id 快照**：`const ids = [...selectedPhotos.value]`（albums:175）、`const album = selectedAlbum.value`（albums:199）→ 请求期间选择/列表变化不影响本次操作

### 6.3 空状态与加载矩阵

| 场景 | 表现 |
|---|---|
| 相册列表加载失败 | 顶部 `AAlert type=error`（`error`）；表格保留（可能为空），可点 `刷新` 重试 |
| 列表无相册 | antd 表格默认空态（本页无自定义文案） |
| 相册无图片 | 大号 UploadDragger 引导（`拖入图片，或点击开始上传`） |
| 筛选后无结果（grid） | `AEmpty description="没有符合条件的图片"` |
| 筛选后无结果（table） | 表格自带空态（`v-else-if="photoView === 'table'"` 分支永远渲染表格） |
| 详情加载中 | `ASpin tip="加载相册…"`（`!ready && detailLoading`） |
| 详情失败 | `AAlert type=error`（`detailError`）；页头 title 回落 `加载相册` |
| 相册不存在（本地找不到） | `AAlert type=warning`「相册不存在，请返回列表重新选择。」 |
| 有上传任务 | 图片页签顶部 `AAlert type=info` + `查看队列` 按钮 |
| COVER 无封面 | `AEmpty`（`PRESENTED_IMAGE_SIMPLE`）`暂无封面` |
| COVER 选择器无匹配 | `AEmpty description="没有匹配的图片"` |

### 6.4 通知与确认（useAdminNotice.ts:6-13）

```ts
add({ title, description?, color? })  // color: 'error' → error(8s) | 'warning' → warning(4s) | 'success' → success(4s) | 其它 → info(4s)
                                      // 统一 { message: title, description, placement: 'topRight' }
confirm(content: string, danger = false): Promise<boolean>
// modal.confirm({ title: '确认操作', content, okText: '确认', cancelText: '取消', okButtonProps: { danger } })
// onOk → resolve(true)；onCancel → resolve(false)
```

**本页消息文案清单（逐字）**：

- `相册已创建，可以上传图片了`（success）
- `相册资料已保存`（success）
- `不支持此文件：{fileName}`（warning）
- `已删除 {n} 张图片`（success / warning，`description: '{n} 个存储对象将在后台继续清理'`）
- `删除失败，请刷新确认`（error）
- `已设为相册封面`（success）／`封面设置失败`（error）
- `封面保存失败，请刷新确认后重试`（error）
- `已恢复自动封面`／`相册封面已更新`（success）
- `请先完成或取消此相册的上传队列`（warning）
- `已删除「{name}」`（success / warning，`description: '剩余存储文件将在后台继续清理'`）
- `相册顺序已保存`（success）／`顺序保存失败，请刷新后重试`（error）
- `正在提交操作，请稍候`（warning）
- 确认弹窗：`有未保存的修改，确定放弃修改并离开吗？`；`永久删除选中的 {n} 张图片？本地、S3 或 WebDAV 中的对应图片和缓存也会删除，无法撤销。`；`永久删除相册「{name}」及其中全部 {n} 张图片？对应存储文件、封面和本地 ZIP 也会清理，无法撤销。`（后两者 danger）

---

## 7. PageHero / PageHeader / MetricCard

### 7.1 `DashboardPageHeader`（`app/components/dashboard/PageHeader.vue`，9 行）

- **props**：`title: string`（必填）、`description?: string`
- **插槽**：单个默认插槽 → 右侧操作区
- **模板**（HEADER:4-8）
  ```html
  <header class="admin-page-header">
    <div><h1>{{ title }}</h1><p v-if="description">{{ description }}</p></div>
    <div class="admin-page-actions"><slot /></div>
  </header>
  ```
- **样式**（admin.css:9-12）：flex、`align-items: flex-start`、space-between、gap 20px、`margin-bottom: 24px`；`h1` 24px/600/`line-height:1.4`；`p` `rgba(0,0,0,.45)` 14px/1.6 `margin-top:8px`；`.admin-page-actions` flex-wrap + `justify-content: flex-end` + gap 8px；≤768px 变纵向、操作区左对齐
- **用途**：整套后台页面的标题栏（含操作区）。调用点：albums:237、DownloadManager.vue:160、dashboard/index.vue:42、settings/storage.vue:515、settings/general.vue:139、tasks.vue:64。相册页的 title/description 随「列表 / 工作区」两态动态切换

### 7.2 `DashboardPageHero`（`app/components/dashboard/PageHero.vue`，34 行）

- **props**（`withDefaults`，HERO:2-11）
  | prop | 类型 | 默认值 |
  |---|---|---|
  | `eyebrow` | `string?` | `'管理后台'` |
  | `title` | `string`（必填） | — |
  | `description` | `string`（必填） | — |
  | `icon` | `string?`（Iconify 名） | `'tabler:sparkles'` |
- **插槽**：具名 `actions`（右侧按钮组，`v-if="$slots.actions"`）+ 默认插槽（`v-if="$slots.default"`，渲染为 `w-full lg:basis-full` 的整行，适合放统计卡片 / 标签行）（HERO:26-32）
- **结构**：`<section class="flex flex-col gap-4 border-b border-default pb-5 lg:flex-row lg:flex-wrap lg:items-end lg:justify-between">` → 左：40px 图标块（`size-10 rounded-lg border border-default bg-default text-primary shadow-xs` + `<Icon class="size-5"/>`）+ `eyebrow`(text-xs text-muted) + `h1`(mt-0.5 text-2xl font-semibold tracking-tight text-highlighted) + `description`(mt-1.5 max-w-3xl text-sm leading-6 text-muted)
- **依赖**：Nuxt UI / Tailwind v4 语义 token（`border-default`、`text-muted`、`text-highlighted`、`bg-default`）与 `Icon` 组件；暗色由 `--ui-*` 变量体系提供（tailwind.css:6-38）
- **当前用途**：全仓库**没有任何调用点**（仅在 git 历史 `cc3b821` / `b819608` 出现）——属于待复用的展示型组件，与相册页现在使用的 ant-design-vue `DashboardPageHeader` 是**两套视觉体系**

### 7.3 `DashboardMetricCard`（`app/components/dashboard/MetricCard.vue`，41 行）

- **props**（METRIC:2-13）
  | prop | 类型 | 默认值 |
  |---|---|---|
  | `label` | `string` | — |
  | `value` | `string \| number` | — |
  | `icon` | `string`（Iconify 名） | — |
  | `tone` | `'primary' \| 'info' \| 'success' \| 'warning' \| 'neutral'` | `'primary'` |
  | `hint` | `string?` | `''` |
  | `to` | `string?` | `undefined` |
- **动态根元素**（METRIC:25-30）：`<component :is="to ? resolveComponent('NuxtLink') : 'div'" :to="to">` —— 传 `to` 变可点击链接，附 hover 态 `hover:border-accented hover:bg-elevated/40`，并在右侧渲染 `tabler:chevron-right` 箭头（`transition group-hover:translate-x-0.5 group-hover:text-primary`）；不传则是静态卡片
- **tone 映射**（METRIC:15-21）
  ```ts
  primary: 'bg-primary/10 text-primary'
  info:    'bg-info/10 text-info'
  success: 'bg-success/10 text-success'
  warning: 'bg-warning/10 text-warning'
  neutral: 'bg-elevated text-muted'
  ```
- **结构**：卡片 `group flex min-w-0 items-center gap-3 rounded-xl border border-default bg-default p-4 shadow-xs transition` → 40px 圆角图标块（`flex size-10 shrink-0 items-center justify-center rounded-lg` + toneClasses）→ 文本列（`min-w-0 flex-1`）：`label`(`block text-xs font-medium text-muted`) / `value`(`mt-0.5 block truncate text-lg font-semibold text-highlighted`) / `hint?`(`mt-0.5 block truncate text-xs text-muted`)
- **当前用途**：全仓库**没有调用点**，为复用组件

---

## 8. React / HeroUI 落地映射与陷阱清单（重写时逐条核对）

### 8.1 组件映射建议

| Vue / ant-design-vue | React 目标（HeroUI v3 / 自研） | 注意 |
|---|---|---|
| `ATable` + `row-selection` + `preserveSelectedRowKeys` | HeroUI `Table` + `Selection`，或自研表格 | HeroUI **没有** `preserveSelectedRowKeys`，必须自己维护 `string[]` 并实现 §4 的并集/差集语义（`toggleVisibleSelection` 可原样移植） |
| `ATabs` / `ATabPane` | HeroUI `Tabs`（或受控 Tab 列表） | 页签**不能有本地 state**，`selectedKey` 必须由 `searchParams.get('tab')` 派生，`onSelectionChange` 只做导航 |
| `AUpload` / `AUploadDragger` / `before-upload` | 自研隐藏 `<input type="file" multiple>` + drag 事件 | 必须保持「`before-upload` 永远返回 false、文件进全局队列、队列自己发请求」的结构 |
| `AImage` 预览 | 自研 Lightbox | 缩略图 `?v=grid2`、预览用 `/api/photos/{id}/preview` |
| `ASpin` / `AEmpty` / `AAlert` | HeroUI `Spinner` / `CircularProgress` + 自研空态 / `Alert` | 文案逐字保留 |
| `ASelect` / `ARadioGroup` / `ACheckbox` | HeroUI `Select` / `RadioGroup` / `Checkbox` | 「本页全选」需要 indeterminate 视觉 |
| `ADatePicker` + `value-format="YYYY-MM-DD"` | `@internationalized/date` + HeroUI `DatePicker`（或原生 `input[type=date]`） | 对外必须序列化为 `'YYYY-MM-DD'` 字符串或 `null` |
| `AModal` / `APopconfirm` / `notification` / `modal.confirm` | HeroUI `Modal` / 自研 Popconfirm / 自研 Toast / ConfirmDialog | 保留 `locked` 期间不可遮罩 & ESC 关闭；`confirm()` 返回 Promise<boolean> |

### 8.2 视觉语义 token 映射（两套体系不同，容易漏）

- Vue 侧：antd 主题 token（`colorPrimary: '#1677ff'`、`borderRadius: 6`，layouts/dashboard.vue:52）+ `admin.css` 里的硬编码色值（`#1677ff`、`#91caff`、`#f5faff`、`#ad6800`、`#cf1322`…）
- `PageHero` / `MetricCard` 侧：Nuxt UI 语义类（`border-default`、`bg-default`、`bg-elevated`、`text-muted`、`text-highlighted`、`text-dimmed`、`border-accented`、`bg-primary/10`、`text-primary`、`text-info`、`text-success`、`text-warning`、`shadow-xs`）—— 在 HeroUI + Tailwind v4 里**不存在同名类**，需要映射为 HeroUI 语义色（如 `border-divider` / `bg-content1` / `bg-content2` / `text-default-500` / `text-foreground` / `text-primary` / `bg-primary/10`）后重写这两个组件
- 现有 `admin.css` 的类名（`.admin-toolbar`、`.admin-photo-grid`、`.admin-photo-tile`、`.admin-selection-bar`、`.admin-save-bar`、`.admin-album-cell`…）应与组件一起迁移，或逐条翻译成 Tailwind 工具类；**断点数值必须一致**（600 / 640 / 768 / 991 / 1024 / 1100 / 1360）

### 8.3 最易漏点（逐条核对）

1. `accept` 与校验正则里的 **`jepg` 拼写**（albums:47,168）与 COVER 正则不含 `jepg`（COVER:51）的不一致——照抄。
2. 上传字段名不同：照片用 **`files`**（useAdminUploads.ts:13），封面用 **`file`**（COVER:56）。
3. **默认页签 `photos` 不写 `tab`**；非法 `tab` 静默回落但不清洗 URL；导航一律 **`push`**（albums:17,109）。
4. 创建相册请求体**不含日期字段**；保存资料是**全量 5 字段**（含三个可空日期），且 name/description 都 `trim()`（albums:147,160）。
5. `dirty` 基于 `JSON.stringify` 比较，**key 顺序敏感**（albums:31,71）。
6. `orderDirty` 用 **id 序列 `join()`** 比较；保存顺序要求「当前全部相册且不重复」（albums:39；main.rs:4151-4162）。
7. 照片删除按钮的怪异判定 `:disabled="locked && mutation !== 'delete-photos'"`（albums:269）。
8. 网格里的 `封面` 标签只在 **`coverPhotoId === photo.id`** 时出现（upload 来源的封面不会给任何图片打标签，albums:274）。
9. `设为封面` 要求**恰好 1 张**选中（albums:187）。
10. 封面 URL 必须用接口返回值（upload 来源每次换 UUID，旧 URL 立即 404）（album_covers.rs:62-78,257-281）。
11. 切相册重置 `photoQuery/photoFormat/photoPage` 但**不重置 `photoSort`**；`photoView` 是**跨页面共享**状态（albums:43,122-125）。
12. `DashboardAlbumCoverEditor` / `DashboardDownloadManager` 都带 `:key="selectedId"` → React 里需要 `key={selectedId}` 强制重建（丢弃内部 state）。
13. 上传队列必须保持「全局单队列 + 并发 7 + 成功后 `albumVersions++` + 1s 去抖刷新 + 绝不中断进行中的请求」（admin-upload-queue.ts:25-51、albums:132-136）。
14. 写操作必须带 `X-Requested-With: ChronoFrame` 与（有 cookie `cf_csrf` 时）`X-CSRF-Token`，`credentials: 'include'`；401 → 全局登出（API:102-126）。
15. `base: '/dashboard/'`：react-router 需要 `basename="/dashboard"`，路由 `/albums`，但**对外 URL 仍是 `/dashboard/albums?album=…&tab=…`**（admin/vite.config.ts:10）。
16. 删除相册/图片被占用时后端 409，消息需原样展示（`所选图片正被转换任务或旧图清理任务使用，请先结束相关任务` 等，main.rs:4552-4585）。
17. 详情请求的竞态保护必须复刻：serial 号 + `selectedId` 复核 + 卸载标记（albums:82-98,138），否则切换相册会出现旧数据覆盖。
