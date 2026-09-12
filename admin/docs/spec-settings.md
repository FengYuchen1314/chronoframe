# ChronoFrame 管理后台两页 React 重写实现规格（基于 Vue 源码逐行摘录）

已完整读取：`app/pages/dashboard/settings/storage.vue`(586行)、`app/pages/dashboard/settings/general.vue`(162行)、`app/composables/useAdminApi.ts`(222行)、`app/types/dashboard.ts`(193行)，并交叉核对 `backend/src/main.rs` 的真实路由与 DTO 以保证 API 契约精确。**全程只读，未修改任何文件。**

---

# 第 0 部分：两页共用的基础设施

## 0.1 `useAdminApi().adminFetch`（useAdminApi.ts:199-212）
- 若 `authState.checked === false || authState.loading === true`：先 `await refreshAuthStatus()`（GET `/api/auth/status`）（:203-205）。
- 若 `!authenticated`：**抛出 `new Error('请先登录管理员账号')`**（:207-208）——不是 HTTP 错误、没有 status。
- `$fetch`，默认头 `X-Requested-With: ChronoFrame`（:108）。
- 非 GET/HEAD 时读 cookie `cf_csrf`，存在则加 `X-CSRF-Token`（:110-113）。
- `credentials: 'include'`（:120）。
- 响应 401（`error.response?.status ?? error.statusCode ?? error.status`，:59-63）→ `markUnauthenticated()` 后重抛（:123-124）。

## 0.2 `getAdminApiErrorMessage(error)`（:65-76）
优先级：字符串本身 → `error.data.error` → `error.data.message` → `error.message` → `'请求失败，请稍后重试'`。

## 0.3 `useAdminNotice()`（useAdminNotice.ts:3-14）
- `add({title, description?, color?})`：color `error/warning/success` → 对应类型，否则 `info`；`notification[type]({message:title, description, placement:'topRight', duration: type==='error' ? 8 : 4})`（秒）。
- `confirm(content, danger=false)`：`modal.confirm({title:'确认操作', content, okText:'确认', cancelText:'取消', okButtonProps:{danger}})`；ESC/点 X 也走 `onCancel → false`。
- **两页所有 confirm 都用默认 `danger=false`**，包括"删除旧存储图片""确认删除旧对象"（storage.vue:331/385/386）。

## 0.4 页面骨架
- 均 `definePageMeta({ layout: 'dashboard' })`（storage:13-15、general:5-7）；layout 提供 `AConfigProvider(zhCN)` + `AApp`（layouts/dashboard.vue:52-86）。
- `useHead` 标题与页面 H1 不同：storage `'存储设置'` vs H1「存储与维护」；general `'站点设置'` vs H1「网站设置」。
- `DashboardPageHeader`（PageHeader.vue:1-9）= `header.admin-page-header > div(h1 + p) + div.admin-page-actions > slot`。
- CSS（admin.css）：`.admin-form-grid` 2 列 `minmax(0,1fr)`、`column-gap:24px`（:15）；`.admin-help` `rgba(0,0,0,.45)` / 13px / 1.7（:17）；**`@media(max-width:768px)`** 下 grid 变 1 列、页头变纵向（:62）。
- 后端这些端点**不**强制 `X-Requested-With`（`require_admin` 只校验 session + 可选 CSRF，main.rs:2248-2276），但前端一律发送。

---

# 第 1 部分：storage.vue（586 行）

## 1.1 静态常量
```ts
// :22-26
backendOptions = [
  { label:'本地存储', value:'local',  icon:'tabler:server' },
  { label:'WebDAV',   value:'webdav', icon:'tabler:cloud-upload' },
  { label:'S3 对象存储', value:'s3',   icon:'tabler:brand-aws' },
]
// :28-32
backendIcons = { local:'tabler:server', webdav:'tabler:cloud-upload', s3:'tabler:brand-aws' }
// :34-38
backendDescriptions = {
  local:'随 Compose 数据目录一起备份迁移',
  webdav:'连接支持 WebDAV 的网盘或服务器',
  s3:'兼容 AWS S3、Cloudflare R2 等对象存储',
}
```
**关键**：`ARadioGroup` 用 `:options` 且**无 `#label` 插槽**（:522），ant 只渲染 `label`/`value` ⇒ **所有 `icon` 从未渲染**；`backendIcons`、`backendDescriptions` **完全未被引用**（死代码）。

## 1.2 表单状态与默认值（:40-79）
```ts
form = reactive({
  backend:'local', localPath:'./data/storage',
  webdavUrl:'', webdavUsername:'', webdavPrefix:'chronoframe',
  s3Endpoint:'', s3Region:'us-east-1', s3Bucket:'', s3AccessKey:'', s3Prefix:'chronoframe',
})
```
独立 ref：`webdavPassword=''`(:53)、`s3SecretKey=''`(:54)、`webdavPasswordSet=false`(:55)、`s3SecretKeySet=false`(:56)、`savedBackend='local'`(:57)、`savedSignature=''`(:58)、`savedTargetSignature=''`(:59)。
9 个 loading/action 布尔（:60-68，均 false）：`isLoading`、`isTesting`、`isSaving`、`isLoadingMigrations`、`isStorageTaskAction`、`isLoadingThumbnailJob`、`isThumbnailTaskAction`、`isLoadingS3Cleanup`、`isS3CleanupAction`。
数据：`migrationJobs=[]`(:69)、`latestThumbnailJob=null`(:70)、`latestS3Cleanup=null`(:71)、`storedPhotoCount=0`(:72)；错误 4 个 `''`(:73-76)；`lastTest=null`(:77)。
**非响应式**：`let maintenancePoll=null`(:78)、`let pageMounted=false`(:79)。

## 1.3 派生状态
- **`formSignature`(:81-92)** = `JSON.stringify` 的 10 字段：`backend, localPath, webdavUrl, webdavUsername, webdavPrefix, s3Endpoint, s3Region, s3Bucket, s3AccessKey, s3Prefix`（**不含两个密码**）。
- **`isDirty`(:94-98)** = `formSignature !== savedSignature || Boolean(webdavPassword) || Boolean(s3SecretKey)`（"密码框有字"即脏）。
- **`targetSignature`(:100-118)** 全部 `.trim()`：local `{backend,localPath}`；webdav `{backend,url,prefix}`（**无 username/password**）；s3 `{backend,endpoint,region,bucket,prefix}`（**无 accessKey/secretKey**）。
- **`storageTargetChanged`(:120-122)** = `targetSignature !== savedTargetSignature`。
- **`latestMigration`(:123)** = `migrationJobs[0] || null`。
- **`activeStorageTask`(:124-126)** = `find(j => ['queued','running'].includes(j.status) || j.cleanupStatus==='cleaning')`。
- **`s3CleanupActive`(:127)** = `latestS3Cleanup?.status === 'running'`。
- **`storageBusy`(:128)** = `Boolean(activeStorageTask || s3CleanupActive)` —— 锁定整个连接表单的总闸。
- **`migrationRequired`(:129)** = `storageTargetChanged && storedPhotoCount > 0`。
- **`migrationProgress`(:130-134)** = `total ? min(100, round(completed/total*100)) : 0`。
- **`thumbnailTaskActive`(:135-137)** = `['queued','running'].includes(status)`。
- **`thumbnailProgress`(:138-142)** / **`s3CleanupProgress`(:143-147)** = `total ? min(100, round(...)) : (status==='completed' ? 100 : 0)`。
- **`lastTestDescription`(:149-154)** = `` `${backendOptions 中该 backend 的 label || backend} · ${at.toLocaleTimeString('zh-CN',{hour12:false})}` ``。

## 1.4 关键函数
- **`clearSensitiveInputs()`(:156-159)** 清空两个密码；调用点 7 处：`applySettings`(:175)、test 请求前(:405)/finally(:427)、save 请求前(:440)/finally(:472)、`changeBackend`(:478)、`onBeforeUnmount`(:501)。
- **`applySettings(settings)`(:161-178)** 服务端→表单，**4 处 falsy 兜底**：`localPath || './data/storage'`、`webdavPrefix || 'chronoframe'`、`s3Region || 'us-east-1'`、`s3Prefix || 'chronoframe'`；`webdavUrl/webdavUsername/s3Endpoint/s3Bucket/s3AccessKey` 直接赋值；同步 `webdavPasswordSet`/`s3SecretKeySet`/`savedBackend`；`clearSensitiveInputs()`；最后 `savedSignature = formSignature; savedTargetSignature = targetSignature`。
- **`buildPayload()`(:180-193)** 全字段 `.trim()`，密码 `webdavPassword.value || undefined`、`s3SecretKey.value || undefined` ⇒ `JSON.stringify` 丢键 ⇒ 后端 `Option<String>=None` ⇒ **保持原值**。
- **`loadSettings()`(:195-213)** 重入保护；`isLoading=true; loadError=''; lastTest=null`；`Promise.all([GET /api/settings/storage, GET /api/albums])`；`applySettings`；`storedPhotoCount = Σ album.photoCount`。
- **`loadMigrations()`(:215-226)** / **`loadThumbnailJob()`(:228-239)** / **`loadS3Cleanup()`(:241-252)**：各自重入保护 + 成功清错 + catch 写各自错误 ref + finally 复位。

## 1.5 状态文案/颜色映射（1:1 复刻）
- **`thumbnailStatusText`(:254-265)**：`'尚未手动重建'`；running→`phase==='clearing' ? '正在清空缓存' : '正在并发生成'`；`queued:'等待开始'`、`completed:'重建完成'`、`failed:'部分生成失败'`、`cancelled:'已安全中断'`、`interrupted:'服务重启后待恢复'`。
- **`thumbnailStatusColor`(:267-274)**：default / success(completed) / error(failed) / warning(cancelled|interrupted) / processing(其余)。
- **`s3CleanupStatusText`(:306-317)**：`'尚未扫描'`；running→`phase==='scanning' ? '正在扫描对象' : '正在并发清理'`；ready→`total ? '等待确认清理' : '空间干净'`；`completed:'清理完成'`、`failed:'任务失败'`、`cancelled:'已安全中断'`、`interrupted:'服务重启后待继续'`。
- **`s3CleanupStatusColor`(:319-326)**：`completed` 或 (`ready` 且 `total===0`)→success；failed→error；`cancelled|interrupted|ready`→warning；其余→processing。
- **`migrationStatusText`(:351-371)**：`cleanupStatus==='cleaning'` 最高优先→`'正在清理旧存储'`；`status==='completed'` 时查 cleanupStatus（`not_ready:'迁移完成'`/`pending:'等待处理旧存储'`/`cleaning`/`cleaned:'旧存储已清理'`/`retained:'旧存储已保留'`/`failed:'旧存储清理失败'`/`interrupted:'旧存储清理已中断'`）；否则查 status（`queued:'等待开始'`/`running:'正在迁移'`/`failed:'迁移失败'`/`cancelled:'迁移已中断'`/`interrupted:'迁移被重启中断'`）。
- **`migrationStatusColor`(:373-378)**：`cleaned|retained`→success；`failed`→error；`cancelled|interrupted`→warning；否则 processing。
- **`formatBytes`(:298-304)**：`<=0`→`'0 B'`；`unit=min(floor(log/log1024),4)`；`value>=100||unit===0 ? toFixed(0) : toFixed(1)`。

## 1.6 表单动作
- **`runThumbnailTaskAction(action)`(:276-296)**：guard `isThumbnailTaskAction`；`start`→`POST /api/thumbnails/rebuilds`，否则 `POST /api/thumbnails/rebuilds/${job?.id}/${action}`（无 body）。toast title：`'三层派生图开始重建'`/`'派生图重建已继续'`/`'已请求安全中断'`；description：cancel→`'正在停止尚未开始的项目，已完成的派生图会保留。'`，其它→`'任务在后端并发运行，可以离开此页面。每张图片会生成完整三层。'`；color cancel→warning 否则 success。成功后 `loadThumbnailJob()`；catch→`'派生图任务操作失败'`。
- **`runS3CleanupAction(action)`(:328-349)**：**guard 在 confirm 之前**；`delete` 确认文案：
  `确定删除扫描到的 ${job.total} 个 S3 旧对象吗？\n\n预计释放 ${formatBytes(job.bytesFound)}。只会处理 ${job.managedPrefix}，删除前还会重新核对数据库引用；删除不能撤销。`
  端点 `POST /api/s3-cleanups/scan` 或 `POST /api/s3-cleanups/${job?.id}/${action}`。toast：scan→`'S3 旧空间扫描已开始'`（desc `'只扫描 ChronoFrame 管理前缀；24 小时内的新对象不会进入清理清单。'`）、delete→`'S3 旧对象开始清理'`（desc `'任务在后端以 8 并发运行，可以离开此页面。'`）、resume→`'S3 任务已继续'`、cancel→`'已请求安全中断'`。成功后 `loadS3Cleanup()`；catch→`'S3 空间任务操作失败'`。
- **`runStorageTaskAction(job, action)`(:380-400)**：guard `isStorageTaskAction`；`cleanup` 确认：
  `确定删除迁移前 ${job.sourceBackend.toUpperCase()} 存储中的全部旧图片吗？\n\n系统会逐张校验当前存储中的副本后再删除，但删除动作不能撤销。`
  `retain` 确认：`确定保留旧存储中的图片吗？\n\n系统会结束本次迁移流程，不会删除旧副本。`
  `POST /api/storage-migrations/${job.id}/${action}`。toast：`'已开始清理旧存储'`/`'已保留旧存储'`/`'已继续迁移'`/`'已请求安全中断'`，无 description。成功后 `loadMigrations()`；catch→`'存储任务操作失败'`。
- **`testConnection()`(:402-430)**：guard `isTesting || isSaving`；**:404 先 `buildPayload()`，:405 立刻 `clearSensitiveInputs()`**；`lastTest=null`；`POST /api/settings/storage/test` body=payload，期望 `{ok:boolean}`；成功 `lastTest={backend:payload.backend, at:new Date()}` + toast `'存储连接测试通过'`/`'本次测试不会保存配置。'`；catch→`'存储连接测试失败'`；finally 再次清空 + `isTesting=false`。
  ⚠️ 测试成功后密码框已被清空，用户若直接保存将不带密码。
- **`saveSettings()`(:432-475)**：guard `isTesting || isSaving`；`migrationRequired` 时确认
  `确认将 ${storedPhotoCount} 张图片迁移到新的存储位置？\n\n迁移会在后台复制并读回校验；完成后才切换存储。随后请在本页确认删除旧空间，或明确选择保留备份。`
  分支 A（迁移）：`POST /api/storage-migrations`（202）→ toast `'存储迁移已开始'`/`'可以离开此页面；任务会持久化记录进度，服务重启后可手动继续。'` → `loadMigrations()`；**不调 `applySettings`** ⇒ `isDirty`/`migrationRequired` 仍为 true，直到轮询检测到任务结束触发 `loadSettings()`(:487)。
  分支 B：`PUT /api/settings/storage` → `applySettings(saved)` + `lastTest={backend:saved.backend, at:new Date()}` + toast `'存储设置已保存'`/`'后端已验证连接并将该配置设为唯一活动存储。'`。
  catch（共用）→ `'存储设置保存失败'`；finally 清空密码 + `isSaving=false`。
- **`changeBackend(backend)`(:477-481)**：`clearSensitiveInputs(); lastTest=null; form.backend=backend`。**不发请求**，不清 `*Set` 标志。

## 1.7 轮询与生命周期
- **`pollMaintenance()`(:483-491)** 递归 setTimeout：`wasActive=storageBusy` → `Promise.all([loadMigrations, loadThumbnailJob, loadS3Cleanup])` → `if (wasActive && !storageBusy) await loadSettings()` → `interval = document.hidden ? 60_000 : (storageBusy||thumbnailTaskActive ? 5_000 : 30_000)`。
- **`onMounted`(:493-498)**：`pageMounted=true` → `await loadSettings()`（串行，先建立 savedSignature）→ 三个加载 Promise.all → `setTimeout(pollMaintenance, busy ? 5000 : 30000)`。
- **`onBeforeUnmount`(:499-503)**：`pageMounted=false; clearSensitiveInputs(); clearTimeout(maintenancePoll)`。
- **`onBeforeRouteLeave`(:510)**：`() => !isDirty || toast.confirm('存储设置尚未保存，确定放弃修改并离开吗？')`。**无 beforeunload**。

## 1.8 Tab 与路由（:504-509）
`storageTab` 可写 computed：get 白名单 `['migration','cache','cleanup']`，否则 `'connection'`；set 用 `router.replace({query: tab==='connection' ? {} : {tab}})`（`connection` = 空 query，不进历史）。外部跳转来源：`tasks.vue:29/31/34`（tab=migration/cache/cleanup）、`conversions.vue:3`。

## 1.9 模板逐块
**页头(:515)**：`title="存储与维护"`，`description="原图存储配置保存在数据库中；迁移和清理任务在后台运行。"`；右侧 `<ATag color="success">{backendOptions 中 savedBackend 的 label}</ATag>`（显示**已保存**后端）+ `<AButton :loading="isLoading" @click="loadSettings(); loadMigrations(); loadThumbnailJob(); loadS3Cleanup()">刷新</AButton>`（loading 只绑 isLoading，四调用未 await）。:516 `loadError` 红色 alert。

### Tab1 `connection`「存储连接」(:518-544) — `<ACard title="原图存储配置" style="max-width:1060px">`
1. `v-if="lastTest"` 绿色 alert `message="连接测试通过"` `:description="lastTestDescription"`。
2. `<AForm layout="vertical" :model="form" @finish="saveSettings">`，**无 `:rules`**（`required` 只是星号，前端零校验）。
   - 存储类型：`<ARadioGroup :value="form.backend" :options="backendOptions" :disabled="isLoading || isSaving || isTesting || storageBusy" @change="changeBackend($event.target.value)" />`（**受控 + 显式 change**）。
   - `v-if="form.backend==='local'"`：单列 `label="本地存储路径" name="localPath" extra="容器内路径，建议保持在 /app/data 下，随数据目录持久化。"` → `AInput`（无 placeholder）。
   - `v-else-if="form.backend==='webdav'"`：`.admin-form-grid`(2列) 4 项 —— `WebDAV URL`(required, placeholder `https://dav.example.com`)、`目录前缀`、`用户名`(autocomplete=off)、`密码`(**无 name**, `:extra="webdavPasswordSet ? '已配置，留空保持不变。' : '首次使用请填写。'"`, `AInputPassword autocomplete="new-password"`)。
   - `v-else`(s3)：`.admin-form-grid` 6 项 —— `S3 Endpoint`(required, placeholder `https://account-id.r2.cloudflarestorage.com`, extra `R2 使用账户的 S3 API 地址，不包含桶名。`)、`区域`(required, extra `Cloudflare R2 填 auto`)、`桶名`(required)、`存储前缀`(extra `不要以 / 开头`)、`Access Key`(required, autocomplete=off)、`Secret Key`(**无 name**, 同 `*Set` 动态 extra, `AInputPassword autocomplete="new-password"`)。
   - 提示(**互斥**)：`v-if="storageBusy"` warning `存储任务正在运行，请完成或中断任务后再修改连接。`；`v-else-if="migrationRequired"` info `'已存在 ' + storedPhotoCount + ' 张图片，将先复制校验，再切换到新存储。'`。
   - `<ASpace wrap>` 顺序：① primary submit `:loading="isSaving" :disabled="isTesting || !isDirty || storageBusy"`，文案 `migrationRequired ? '开始安全迁移' : '保存并启用'`；② `测试连接` `:loading="isTesting" :disabled="isSaving || storageBusy"`；③ `重置` `:disabled="!isDirty || isSaving"`（= 重新 GET，**非恢复默认**）。
   - 页脚 `.admin-help`：`测试不会保存配置。密码和 Secret Key 发送后立即清空，不会写入浏览器存储。相册 ZIP 与此处配置无关，始终保存在本地。`

### Tab2 `migration`「存储迁移」(:545-561)
`migrationLoadError` warning alert → 无 `latestMigration` 时 info `暂无迁移记录。修改存储连接并保存后，会自动创建迁移任务。` → 否则 `<ACard title="最近一次迁移">`：`#extra` 状态 Tag；`ADescriptions :column="3"`（`来源`=sourceBackend.toUpperCase()、`目标`=targetBackend.toUpperCase()、`图片`=total）；`AProgress :percent="cleanupStatus==='cleaning' ? (total ? Math.round(cleanupCompleted/total*100) : 0) : migrationProgress"`（**切分子、不夹取 100**）；`.admin-help` `成功 {succeeded} · 失败 {failed} · 旧对象已清理 {cleanupCompleted}`；`error` warning alert；`status==='completed' && ['pending','failed','interrupted'].includes(cleanupStatus)` 时 warning `新存储已启用，请决定是否删除旧存储中的副本。`。
按钮 `<ASpace wrap>`（4 个，共享 `isStorageTaskAction`，条件互斥）：

| 按钮 | 显示条件 | 端点 | 样式 |
|---|---|---|---|
| 安全中断 | `status∈(queued,running) \|\| cleanupStatus==='cleaning'` | `POST /api/storage-migrations/{id}/cancel` | 默认 |
| 继续迁移 | `status∈(failed,cancelled,interrupted)` | `POST .../{id}/resume` | primary |
| 删除旧存储图片 | `status==='completed' && cleanupStatus∈(pending,failed,interrupted)` | 确认后 `POST .../{id}/cleanup` | danger |
| 保留旧副本 | 同上一行（同一 `template v-if`） | 确认后 `POST .../{id}/retain` | 默认 |

### Tab3 `cache`「图片缓存」(:562-570) — `<ACard title="重建三层浏览图">`
`#extra` 状态 Tag；固定说明 `生成 320px PNG、≤1.5 MB WebP 预览和 ≤5 MB WebP 高清图。此操作不改变原图，也不删除下载 ZIP。`（与后端常量一致：`GRID_THUMBNAIL_LONGEST_EDGE=320`、`VIEW_PREVIEW_MAX_BYTES=1_500_000`、`VIEW_HIGH_MAX_BYTES=5_000_000`）；`thumbnailLoadError` warning；有 job 时 `AProgress` + `{completed} / {total} · 成功 {succeeded} · 失败 {failed} · 并发 {workerCount}` + `error` warning。
按钮 `<ASpace>`（**无 wrap**，共享 `isThumbnailTaskAction`）：① `thumbnailTaskActive` → 安全中断（`POST /api/thumbnails/rebuilds/{id}/cancel`）；② `v-else` primary `:disabled="storageBusy"` → 清空并重新生成（`POST /api/thumbnails/rebuilds`）；③ `status∈(failed,cancelled,interrupted)` → 继续上次任务（`POST .../resume`）。

### Tab4 `cleanup`「S3 空间清理」(:571-583) — `<ACard title="清理失去引用的旧对象">`
`#extra` 状态 Tag；**常驻** info alert `message="先扫描，再由管理员确认删除"` `description="仅处理 ChronoFrame 管理前缀，保护数据库引用和 24 小时内的新对象。不会删除本地 ZIP。"`（后端 `S3_ORPHAN_GRACE_SECONDS = 24*60*60`）；`s3CleanupLoadError` warning；有 job 时 `ADescriptions :column="3"`（`已扫描对象`=scannedObjects、`候选旧对象`=total、`预计释放`=formatBytes(bytesFound)）+ `AProgress` + `.admin-help` `已删除 {deleted} · 已释放 {formatBytes(bytesDeleted)} · 失败 {failed}` + `error` warning。
按钮 `<ASpace wrap>`（共享 `isS3CleanupAction`）：① `s3CleanupActive` → 安全中断；② `v-else` primary `:disabled="savedBackend !== 's3' || storageBusy"` → 扫描旧对象（**用 `savedBackend` 而非 `form.backend`**）；③ `status==='ready' && total>0` → danger 确认删除旧对象；④ `status∈(failed,cancelled,interrupted)` → 继续任务。
该 Tab 内容**不因非 s3 后端隐藏**，只禁用扫描按钮。

## 1.10 API 调用清单（精确）
| # | 触发 | Method | 路径 | Body | 期望响应 | 行 |
|---|---|---|---|---|---|---|
| 1 | mount/重置/刷新/轮询 | GET | `/api/settings/storage` | — | `StorageSettings` | :203 |
| 2 | 同上并行 | GET | `/api/albums` | — | `Album[]`（只用 photoCount） | :204 |
| 3 | mount/轮询/刷新/动作后 | GET | `/api/storage-migrations` | — | `StorageMigrationJob[]`（后端 `ORDER BY created_at DESC LIMIT 50`） | :219 |
| 4 | 同上 | GET | `/api/thumbnails/rebuilds/latest` | — | `ThumbnailRebuildJob \| null` | :232 |
| 5 | 同上 | GET | `/api/s3-cleanups/latest` | — | `S3CleanupJob \| null` | :245 |
| 6 | 测试连接 | POST | `/api/settings/storage/test` | `StorageSettingsInput` | `{ok:true}` | :410 |
| 7 | 保存（非迁移） | PUT | `/api/settings/storage` | `StorageSettingsInput` | `StorageSettings` | :453 |
| 8 | 保存（迁移） | POST | `/api/storage-migrations` | `StorageSettingsInput` | **202** `StorageMigrationJob` | :445 |
| 9 | 继续迁移 | POST | `/api/storage-migrations/{id}/resume` | 无 | 202 空体 | :389 |
| 10 | 中断迁移 | POST | `.../{id}/cancel` | 无 | 202 空体 | :389 |
| 11 | 清理旧存储 | POST | `.../{id}/cleanup` | 无 | 202 空体 | :389 |
| 12 | 保留旧副本 | POST | `.../{id}/retain` | 无 | **204** 空体 | :389 |
| 13 | 开始重建 | POST | `/api/thumbnails/rebuilds` | 无 | 202 `ThumbnailRebuildJob` | :282 |
| 14/15 | 继续/中断重建 | POST | `/api/thumbnails/rebuilds/{id}/resume\|cancel` | 无 | 202 空体 | :283 |
| 16 | 扫描 | POST | `/api/s3-cleanups/scan` | 无 | 202 `S3CleanupJob` | :335 |
| 17/18/19 | 删除/继续/中断 | POST | `/api/s3-cleanups/{id}/delete\|resume\|cancel` | 无 | 202 空体 | :336 |

**#9-#19 全为空响应体，不要假设 JSON。** 路由表来源 main.rs:6839-6875、6912-6924。

**请求体 `StorageSettingsInput`**（dashboard.ts:170-183；后端 field 除 `backend` 外全为 `Option<String>`，main.rs:771-786）：
```jsonc
{"backend":"local|webdav|s3","localPath":"","webdavUrl":"","webdavUsername":"",
 "webdavPassword":"…","webdavPrefix":"","s3Endpoint":"","s3Region":"",
 "s3Bucket":"","s3AccessKey":"","s3SecretKey":"…","s3Prefix":""}
```
**响应 `StorageSettings`**（dashboard.ts:155-168；main.rs:882-897）：同名字段但两个密钥换成 `webdavPasswordSet: boolean` / `s3SecretKeySet: boolean`。**`s3AccessKey` 是回显的**。

`StorageMigrationJob`（:99-116 / main.rs:1549-1568）：`id, status(6态), sourceBackend, targetBackend, total, completed, succeeded, failed, cancelled, cleanupStatus(7态), cleanupCompleted, cleanupFailed, createdAt, updatedAt, activatedAt|null, error|null`。
`ThumbnailRebuildJob`（:118-133 / main.rs:1570-1587）：`id, status, phase('queued'|'clearing'|'generating'), total, completed, succeeded, failed, skipped, cancelled, cacheFilesRemoved, workerCount, createdAt, updatedAt, error|null`。
`S3CleanupJob`（:135-153 / main.rs:1589-1609）：`id, status('running'|'ready'|'completed'|'failed'|'cancelled'|'interrupted'), phase('scanning'|'ready'|'deleting'), scannedObjects, protectedObjects, total, completed, deleted, failed, skipped, bytesFound, bytesDeleted, workerCount, managedPrefix, createdAt, updatedAt, error|null`。

## 1.11 敏感字段（第 3 问）
1. GET 只返回 `webdavPasswordSet`/`s3SecretKeySet` 布尔，密码框初值永远为空。
2. 提示随标志变化（:528/:536）：true→`'已配置，留空保持不变。'`，false→`'首次使用请填写。'`。
3. **留空 = 保持原值**，靠 `|| undefined` 丢键 + 后端 `None` 沿用；**没有清空密钥的路径**。
4. 页脚明确提示 `密码和 Secret Key 发送后立即清空，不会写入浏览器存储。`（:541），实现确实如此。
5. `isDirty` 把"密码框非空"独立视为脏（:96-97）。
6. 密码 `AFormItem` **无 `name`**，不参与表单 model。
7. 切换 backend 清空输入但**不清** `*Set` 标志。

## 1.12 三后端差异（第 4 问）
| 维度 | local | webdav | s3 |
|---|---|---|---|
| 字段 | `localPath` 单列+extra | URL(required/placeholder)、目录前缀、用户名、密码（2列） | Endpoint(required/placeholder/extra)、区域(required/extra R2 auto)、桶名(required)、前缀(extra 不要以 / 开头)、Access Key(required)、Secret Key（2列，顺序固定） |
| 默认 | `./data/storage` | prefix `chronoframe` | region `us-east-1`、prefix `chronoframe` |
| targetSignature | backend+localPath | backend+url+prefix | backend+endpoint+region+bucket+prefix |
| 后端校验(main.rs:823-856) | 路径非空 | URL 可解析且 http/https、username/password 非空、prefix 合法 | Endpoint 同上、region/bucket/accessKey/secretKey 非空、prefix 合法 |
| S3 清理扫描 | 禁用 | 禁用 | 可用 |

`PUT` 在迁移/清理运行中返回 **409** `存储迁移或旧存储清理正在运行，暂时不能修改配置`（main.rs:2600-2605）——正是 `storageBusy` 的镜像。

## 1.13 连接测试（第 5 问）
顺序：guard `isTesting||isSaving` → `buildPayload()`(:404) → **立刻清空密码框**(:405) → `isTesting=true; lastTest=null` → `POST /api/settings/storage/test`（后端 main.rs:2614-2627：`require_admin(true)` + storage 读锁 + `test_candidate`，失败 400 `存储连接测试失败：{error:#}`，**不写任何配置**）→ 成功设 `lastTest`（页头下绿条 `连接测试通过` + `{后端中文标签} · HH:mm:ss`）与 success toast `本次测试不会保存配置。` → 失败 error toast（表单值保留）→ finally 再次清空 + `isTesting=false`。`lastTest` 只在 `loadSettings`(:199)、`changeBackend`(:479)、再次测试 时清除。

## 1.14 迁移/清理/重建按钮与后端错误对齐（第 6 问）
`storageBusy` 禁用：backend 单选、保存、测试连接、缩略图「清空并重新生成」、S3「扫描旧对象」，并显示警告条。缩略图任务**不在** `storageBusy` 内（独立 `thumbnailTaskActive`，只改轮询间隔）。迁移 Tab 只渲染第 0 条。
后端约束（用于文案对齐）：重复迁移 409 `已有未收尾的存储迁移；请继续任务并选择清理或保留旧存储`；有转换/队列 409 `请先结束图片转换并等待对象清理队列完成，再开始迁移`；源=目标 400 `源存储和目标存储位置相同，无需迁移`；无照片 400 `当前没有图片，请直接保存新的存储配置`；目标探测失败 400 `目标存储连接测试失败：…`（main.rs:2915-2971）。cancel 非运行 400 `任务不在运行中，无法中断`（:3072）。cleanup 条件不符 400 `此任务当前不能清理旧存储`；目标位置变 409 `当前存储位置已改变，无法确认迁移目标，拒绝清理旧存储`；源=当前 409 `旧存储与当前存储相同，拒绝删除`（:3234-3257）。retain 不符 400 `此任务当前不能选择保留旧存储`（:3286）。S3 scan 非 s3 400 `只有当前活动存储为 S3 时才能扫描旧空间`，重复 scan 会把旧 ready 置 `cancelled`（error=`已被新的扫描结果替代`）；delete 未 ready 400 `请先完成一次 S3 旧空间扫描`，位置变 409 `S3 存储位置已改变，请重新扫描后再清理`；resume 不符 400 `此 S3 清理任务当前不能继续`；cancel 非运行 400 `S3 扫描或清理任务不在运行中`。缩略图：重复 409 `已有缩略图重建任务正在运行`；存储锁被占 409 `存储迁移或清理正在运行，请稍后重建缩略图`；cancel 非运行 400 `缩略图重建任务不在运行中`；resume 不符 400 `此缩略图任务当前不能继续`。

## 1.15 loading/校验/错误/未保存（第 8 问，storage）
- Loading：无骨架屏，用 `AProgress`/`ATag` 表达；迁移 Tab 在 `isLoadingMigrations` 期间**无任何 loading 指示**（只显示旧数据）；三组动作按钮各自共享一个 action 标志（同时转圈）。
- **前端零校验**：无 `rules`，全靠后端中文错误 → error toast。
- 错误分支：首屏失败 → 页头红 alert，且 `savedSignature=''` ⇒ 初始 `isDirty===true`（保存/重置按钮可用），属原实现边界行为；三 Tab 各自独立 warning alert，互不影响；动作失败不回滚表单；401 → authState 失活 → 登录门；未登录时 `adminFetch` 抛 `请先登录管理员账号` 直接作为 toast 文案。
- 未保存提示：仅 `onBeforeRouteLeave`；**无 beforeunload**；迁移发起成功后 `isDirty` 仍为 true，立即离开仍会提示。

---

# 第 2 部分：general.vue（162 行）

## 2.1 状态
```ts
// :16-20（icon 同样从未渲染）
themeOptions = [{label:'跟随系统',value:'system',icon:'tabler:device-desktop'},
                {label:'浅色',value:'light',icon:'tabler:sun'},
                {label:'深色',value:'dark',icon:'tabler:moon'}]
// :22-28
form = reactive<SiteSettings>({ title:'ChronoFrame', slogan:'Frame the moments that matter.',
  author:'ChronoFrame', avatarUrl:'/web-app-manifest-192x192.png', theme:'system' })
savedSignature=''; isLoading=false; isSaving=false; loadError=''; avatarPreviewFailed=false  // :29-33
```
依赖：`useAdminNotice()`(:11)、`useColorMode()`(:12，nuxt color-mode，`storageKey:'cframe-color-mode'`)、`adminFetch`(:13)、`useSiteSettings().applySiteSettings`(:14，全局 state `chronoframe-site-settings`，useSiteSettings.ts:14-32)。
**死代码**：`avatarPreviewFailed` 只被写入（:51/:147）**从未被模板读取** ⇒ 预览头像无 onError 兜底；`changeTheme`(:74-76) **从未被调用**（模板用 `v-model:value`，:148）。

## 2.2 派生
`formSignature`(:35-41) = `{title,slogan,author,avatarUrl,theme}` 的 JSON；`isDirty`(:42) = 与 `savedSignature` 比较；`previewAvatarUrl`(:43) = `avatarUrl.trim() || '/web-app-manifest-192x192.png'`。

## 2.3 函数
- `applyForm`(:45-53)：逐字段覆盖（**无 trim / 无兜底**），`avatarPreviewFailed=false`，`savedSignature=formSignature`。
- `loadSettings`(:55-68)：guard `isLoading`；`GET /api/settings/site` → `applyForm` + **`applySiteSettings`**（同步公开站点状态，内含 trim 与 theme 白名单兜底）；**不设 `colorMode.preference`**。
- `resetForm`(:70-72) = `void loadSettings()`（与服务端对齐，非恢复默认）。
- `saveSettings`(:78-120)：guard `isSaving || !isDirty` → `title=trim`，空则 warning toast `网站名称不能为空`（无 description）→ 码点长度校验 `title>100 || slogan.trim()>200 || author.trim()>100`（`Array.from(...).length`）→ warning `站点文字超出长度限制` → `put body={title, slogan:trim, author:trim, avatarUrl:trim, theme}` → `applyForm` + `applySiteSettings` + **`colorMode.preference = settings.theme`**(:105) → success toast `站点设置已保存`/`公开页面已立即使用新的名称、标语、作者、头像和默认主题。` → catch error toast `保存站点设置失败`。
- `confirmDiscardChanges`(:122) = `!isDirty || toast.confirm('站点设置尚未保存，确定要放弃修改吗？')`；`handleBeforeUnload`(:123-127) 在 dirty 时 `preventDefault(); returnValue=true`。
- 生命周期：`onBeforeRouteLeave`(:129)、`onMounted` 注册 beforeunload + `loadSettings()`(:130-133)、`onBeforeUnmount` 移除(:134)。

## 2.4 模板
页头(:139) `title="网站设置"` `description="配置公开页面的网站名称、标语、作者、头像和默认主题。"` + 右侧 `重新读取`（`:loading="isLoading" :disabled="isSaving"`）；`loadError` 红 alert(:140)。
布局(:141)：`grid gap-6 xl:grid-cols-[minmax(0,1fr)_320px]` —— 默认单列，**≥1280px(xl) 两列，右列固定 320px**。
左卡片「基本设置」(:142-152)，`AForm layout="vertical" @finish="saveSettings"`（**无 rules**）：
① `网站名称` name=title **required** maxlength=100；② `网站标语` name=slogan extra=`留空可隐藏` maxlength=200；③ `作者名称` name=author maxlength=100（无 required/extra）；④ `头像 URL` name=avatarUrl extra=`支持站内路径或完整 HTTP(S) 地址` maxlength=2048 `@change="avatarPreviewFailed=false"`；⑤ `默认主题` name=theme `<ARadioGroup v-model:value="form.theme" :options="themeOptions" :disabled="isLoading||isSaving" />`（**双向绑定，与 storage 页写法不同**）。全部 `AInput` `:disabled="isLoading || isSaving"`。
按钮：`保存设置` primary submit `:loading="isSaving" :disabled="!isDirty || isLoading"`；`重置` `:disabled="!isDirty || isLoading || isSaving"` `@click="resetForm"`。`<p v-if="isDirty" class="admin-help mt-3">有未保存的修改</p>`。
右卡片「站点预览」(:153-159)：`<AAvatar :size="64" shape="square" :src="previewAvatarUrl" />`（**无 onError 回退**）；`<h2>{{ form.title || '网站名称' }}</h2>`；`<p class="admin-help">{{ form.slogan || '这里显示网站标语' }}</p>`；`<p class="admin-help">© {{ form.author || '作者' }}</p>`；`<NuxtLink to="/" target="_blank"><AButton>查看公开页面</AButton></NuxtLink>`。预览用**当前表单值**（实时），非已保存值。

## 2.5 API
| 触发 | Method | 路径 | Body | 响应 |
|---|---|---|---|---|
| mount/重新读取/重置 | GET | `/api/settings/site` | — | `SiteSettings` |
| 保存 | PUT | `/api/settings/site` | `SiteSettings` 5 字段（已 trim） | `SiteSettings`（归一化后） |

`SiteSettings`（dashboard.ts:185-193）：`{title, slogan, author, avatarUrl, theme:'light'|'dark'|'system'}`；后端 camelCase + **`deny_unknown_fields`**（main.rs:905-913，多传键即 400）。`GET` 后端**是公开端点**（main.rs:3837-3839 不鉴权，公开页面共用）；`PUT` 需管理员 + CSRF。
后端校验（main.rs:926-963）：title 非空且 ≤100 码点 / slogan ≤200 / author ≤100 / avatarUrl ≤2048；avatarUrl 非空时必须是「以 `/` 开头、非 `//`、无控制字符」或「可解析且 scheme ∈ http/https」，否则 400 `头像 URL 必须是以 / 开头的站内路径，或完整的 HTTP(S) URL`（**`javascript:` 被拒**）；theme ∈ {light,dark,system}。

## 2.6 头像上传流程（第 7 问明确回答）
**不存在上传流程。** 只有文本「头像 URL」输入 + 预览 `AAvatar`：无文件选择器、无拖拽、无进度、无裁剪、无 `FormData`、无上传端点。预览 URL 空时兜底 `/web-app-manifest-192x192.png`。后端唯一的图片上传是相册封面 `POST /api/albums/{album_id}/cover`（main.rs:6879-6885）与照片 `POST /api/albums/{album_id}/photos`（:6894-6899），与站点头像无关。**React 版不应自行添加文件上传 UI**，除非产品明确要求新增后端能力。

## 2.7 保存语义
全量覆盖 5 字段（PUT 语义）；空 slogan/author/avatarUrl 后端接受。空 avatarUrl 保存后，全局值被 `applySiteSettings` 兜底回默认头像（useSiteSettings.ts:26），但本页表单显示空字符串、预览回落默认图。仅保存成功写 `colorMode.preference`（立即变色），「重新读取」不写。保存成功重置 `savedSignature` ⇒ 脏标记与「有未保存的修改」消失。theme 三项即**站点默认主题**（服务端持久化 + 新访客默认），不是当前用户偏好。

---

# 第 3 部分：React 重写必须保真的差异清单（易漏点速查）

1. 两页保存按钮在无改动时禁用（`!isDirty`），"保存即应用"。
2. storage 页 RadioGroup 是 `:value` + `@change=$event.target.value`；general 页是 `v-model`。两者 `icon` 数据都未渲染。
3. 密码字段**无 `name`、不属 form**，是独立 state，`isDirty` 单独判定。
4. **省略键 = 保持原值**（`|| undefined` → 丢键）；无清空密钥路径；测试/保存成功后输入框被清空（时机分别为"请求前"与"请求前+finally"）。
5. `*Set` 只来自 GET；切换 backend 不清零，只有 `applySettings` 更新。
6. `applySettings` 恰有 4 处 falsy 兜底（localPath/webdavPrefix/s3Region/s3Prefix），URL/用户名/bucket/accessKey 无兜底。
7. 保存分支完全由 `migrationRequired = targetSignature 变 && 照片数>0` 决定（POST 迁移 202 vs PUT）；迁移分支**不**刷新 `savedSignature`。
8. `storedPhotoCount` 来自 `GET /api/albums` 的 `photoCount` 求和，不是 `/api/photos`。
9. `cleanupStatus==='cleaning'` 时迁移进度分子换成 `cleanupCompleted/total`，且**不夹取到 100**。
10. S3 扫描按钮用 `savedBackend` 禁用，页头绿 Tag 也用 `savedBackend`。
11. 轮询是递归 setTimeout，`document.hidden ? 60s : (busy ? 5s : 30s)`；`wasActive && !storageBusy` 时才 `loadSettings()`（清脏标记的唯一自动路径）。
12. storage 的 confirm 在 guard **之后**，S3 的 confirm 在 guard **之前**（顺序不同）。
13. 迁移 4 / 缩略图 3 / S3 4 个按钮各自共享一个 action 标志，会同时 loading。
14. 迁移 Tab 只显示 `migrationJobs[0]`，其余 49 条无 UI。
15. general 页有 `beforeunload` + `onBeforeRouteLeave`；storage 页**只有** `onBeforeRouteLeave`。
16. 死代码：`avatarPreviewFailed`、`changeTheme`、`backendIcons`、`backendDescriptions`、`themeOptions[].icon`；未使用字段：`StorageMigrationJob.cancelled`、`ThumbnailRebuildJob.skipped/cacheFilesRemoved/cancelled`、`S3CleanupJob.protectedObjects/completed/skipped`。
17. 所有空体 202/204 响应不解析 JSON。
18. **无任何前端表单校验规则**；`required`/`maxlength` 是仅有的输入约束，其余靠后端中文错误。
19. `onMounted` 中 `loadSettings` 必须先于三个轮询加载完成（否则 `isDirty` 计算错误）。
