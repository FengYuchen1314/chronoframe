# 管理后台重写规格书 — 上传队列 与 下载管理

> 目标仓库：`/Users/fengyuchen/Desktop/coding/相册集`
> 基准：当前 HEAD。所有行号均已逐条读源码核对。
> 范围：**A 上传队列**、**B 下载管理**为完整规格；**C 格式转换**现状说明（前端已删除，不在重写范围）。
> 本文是重写工作的唯一依据。凡标注「现网行为」之处，重写时应忠实复刻，除非明确决定变更并同步后端。

---

## 0. 关键结论（先读这 6 条）

1. **上传队列没有单文件进度百分比**。进度只有「文件计数」，不是字节数；上传请求既没有 `onUploadProgress`，也没有 `AbortSignal`（全仓库 `app/` `shared/` 无 `signal` / `onUploadProgress` 命中）。
2. **上传不可取消**。源码注释明确拒绝 abort —— `shared/utils/admin-upload-queue.ts:25-26`：
   > `// One shared controller, seven slots across ALL albums. Never abort an upload:`
   > `// a lost response cannot tell us whether the server has already committed it.`
   所谓「取消」只是把 `queued` 项从数组移除。
3. **「7 个并发」在前后端各实现一次**：前端 `createUploadQueue(..., concurrency = 7)`（`admin-upload-queue.ts:31`）；后端 `const DEFAULT_UPLOAD_CONCURRENCY: usize = 7`（`backend/src/main.rs:90`）+ `Semaphore::new(7)`。两者独立，不是同一份状态。
4. **不需要也不能持久化 `File`**：状态在 `useState`（跨页单例），控制器在模块级 `WeakMap`。刷新页面队列即清空 —— **这是设计而非缺陷**，`File` 引用无法序列化。重写时不要试图补 `localStorage`。
5. **`failed` 有语义陷阱，必须保留**：`failed` 不等于「没上传成功」，而是「**没有拿到服务器的成功响应**」。所以重试前必须弹确认框警告可能重复入库（`UploadQueue.vue:10`），且**重试次数根本不记录**（无 counter 字段）。
6. **`DownloadManager.vue` 是双上下文组件**：`embedded` prop 切换两套布局 / 路由来源 / 离开拦截语义。详见 §B.7。

---

# A. 上传队列（UploadQueue + useAdminUploads）

## A.1 文件与所有权

| 文件 | 行数 | 角色 |
|---|---|---|
| `app/components/dashboard/UploadQueue.vue` | 38 | 顶栏按钮 + Drawer UI。挂在 `app/layouts/dashboard.vue:80` 顶栏 |
| `app/composables/useAdminUploads.ts` | 24 | Vue 适配层：`useState` + 模块级 `WeakMap` 控制器缓存 + 5 个 computed |
| `shared/utils/admin-upload-queue.ts` | 74 | **全部队列逻辑，框架无关纯函数**。React 可直接复用，建议原样保留 |
| `app/pages/dashboard/albums.vue` | — | 唯一入队点（`:169`），并监听 `albumVersions` 刷新图片列表 |
| `app/utils/adminFormat.ts` | 8 | `adminBytes()` 字节格式化（1000 进制） |

## A.2 数据结构（精确摘录）

`shared/utils/admin-upload-queue.ts:1-23`：

```ts
export type UploadStatus = 'queued' | 'uploading' | 'done' | 'failed'

export interface UploadItem<T> {
  id: number          // 自增，来自 state.nextId++
  albumId: string     // 入队时快照，之后永不变
  albumName: string   // 入队时快照，仅用于显示（相册改名后此处仍是旧名）
  name: string
  size: number
  file?: T            // 仅成功时被置 undefined 以释放引用
  status: UploadStatus
  error: string
}

export interface UploadQueueState<T> {
  items: UploadItem<T>[]
  paused: boolean
  nextId: number
  albumVersions: Record<string, number>   // albumId -> 成功计数，用于触发外部刷新
}

export function createUploadQueueState<T>(): UploadQueueState<T> {
  return { items: [], paused: false, nextId: 1, albumVersions: {} }
}
```

**`UploadItem` 没有 `progress`、没有 `retryCount`、没有 `aborted` 字段。** 重写时不要「顺手」加进度字段——那会与后端契约和全部 UI 文案都不一致。

派生计数（`useAdminUploads.ts:18-22`）：

```ts
const active  = computed(() => state.value.items.filter(item => item.status === 'uploading').length)
const queued  = computed(() => state.value.items.filter(item => item.status === 'queued').length)
const failed  = computed(() => state.value.items.filter(item => item.status === 'failed').length)
const done    = computed(() => state.value.items.filter(item => item.status === 'done').length)
const pending = computed(() => active.value + queued.value)   // 注意：不含 failed
```

React 对应：`items` 用 `useState` / `useReducer`，4 个计数用 `useMemo`。
**`pending` 不含 `failed`** —— 这点被 `beforeunload` 与「退出登录」拦截依赖。

## A.3 状态机

```
                 enqueue()                    pump() 取到名额
   (不存在) ──────────────► queued ──────────────────────────► uploading
                             ▲                                   │
                             │                          ┌────────┴────────┐
                  retryFailed()（file 仍在）            │                 │
                             │                       成功 │                 │ 抛错
                          failed ◄───────────────────────┘                 │
                             │                                   done ◄───┘
                             │                                     │
              remove(id) ────┴──────────► (移出数组)  ◄──────────────┘
```

转移表：

| 起点 | 动作 | 终点 | 守卫 | 代码行 |
|---|---|---|---|---|
| — | `enqueue(files, album)` | `queued` | 无 | `admin-upload-queue.ts:53-58` |
| `queued` | `pump()` | `uploading` | `!paused` 且有空闲名额 且 `item.file` 存在 | `:33-41` |
| `uploading` | promise resolve | `done` | 置 `file = undefined`；`albumVersions[albumId]++` | `:42-45` |
| `uploading` | promise reject | `failed` | `error = describeError(cause)` | `:46-48` |
| `failed` | `retryFailed()` | `queued` | **仅当 `item.file` 仍存在**，同时清空 `error` | `:62-67` |
| 任意非 `uploading` | `remove(id)` | 移出数组 | `status !== 'uploading'` 才允许 | `:68-71` |
| `done` | `clearDone()` | 移出数组 | 无 | `:72` |

**没有 `cancelled` 状态。** `remove` 是删除记录，不是取消上传。
`finally(pump)`（`:49`）保证每个任务结束后立刻补位 —— 这是队列能自动推进的**唯一机制**，React 重写必须保留，否则队列会卡死。

## A.4 并发控制：7 个名额的精确实现

`admin-upload-queue.ts:33-51`：

```ts
const pump = () => {
  if (state.paused) return                                   // ① 暂停则完全不启动
  let slots = concurrency - state.items.filter(item => item.status === 'uploading').length  // ② 动态算空位
  for (const item of state.items) {                          // ③ 按数组顺序（= 入队顺序）FIFO
    if (slots <= 0) break
    if (item.status !== 'queued' || !item.file) continue
    slots--
    item.status = 'uploading'                                // ④ 同步占位，避免重复启动
    const file = item.file
    void Promise.resolve().then(() => upload(file, item.albumId)).then(() => {
      item.status = 'done'
      item.file = undefined // Release the browser's File reference after success.
      state.albumVersions[item.albumId] = (state.albumVersions[item.albumId] || 0) + 1
    }).catch((cause: unknown) => {
      item.status = 'failed'
      item.error = describeError(cause)
    }).finally(pump)                                          // ⑤ 完成即补位
  }
}
```

要点：

- **7 是跨全部相册共享的单一名额池。** 注释在 `:25`：「One shared controller, seven slots across ALL albums.」多次 `enqueue` 到不同相册时，仍在同一个 7 名额内竞争。
- 名额是**动态扣除**而非独立计数器：每次都重新用 `uploading` 数量反推。因此并发数永不超过 7，即使 `pump` 被重入。
- `slots--` 与 `item.status = 'uploading'` 在**同一个同步循环**内完成，所以虽然 `upload()` 被推到微任务（`Promise.resolve().then`），也不会重复启动同一项。**React 中若用 `await` 或 `useEffect` 异步派发，必须自己保证这个同步占位原子性，否则并发会超限。**
- 顺序：`state.items` 数组顺序。`remove` 用 `splice` 保序，所以是严格 FIFO；失败重试只改 status **不移动位置**，因此重试项会**优先于**后面新入队的项。
- `concurrency` 是函数第 4 个参数默认 7（`:31`），`useAdminUploads.ts:11-15` 调用时**未传该参数**，所以实际取默认值 7。

## A.5 与「全局 / 后台共享」的关系

三层共享，互相独立：

1. **单浏览器内全局共享。** 一个 Nuxt app 只有一份队列。`useState('admin-upload-queue')` 是 Nuxt 全局状态（非组件局部），`controllers` 是模块级 `WeakMap`（`useAdminUploads.ts:3`）。因为 `app/layouts/dashboard.vue:13` 在**布局层**也调用了 `useAdminUploads()`，队列在任何后台页面都存活，**包括不在相册页时**。
2. **服务端也有 7 名额。** `backend/src/main.rs:90` `DEFAULT_UPLOAD_CONCURRENCY = 7`；`:6809` 建 `Semaphore::new(7)`；`upload_photos`（`:4751` 起）逐个 `state.upload_slots.acquire().await`，拿不到就排队；服务关闭时报 `AppError::bad("上传队列已关闭")`。上传期间还持 `storage_mutation_gate.try_read_owned()`，存储迁移/清理中直接 409：`"存储正在迁移或清理，请稍后再上传"`。
3. **不与其它后台任务共享名额。** 缩略图重建、S3 清理、存储迁移、格式转换各有独立并发配置，互不占用上传名额。
4. **跨标签页不共享。** 每个标签页独立内存状态：两个标签页 = 两份队列、共 14 个前端名额（但服务端仍限 7）。

## A.6 上传请求契约

`app/composables/useAdminUploads.ts:11-15`：

```ts
controller = createUploadQueue(state.value, async (file, albumId) => {
  const body = new FormData()
  body.append('files', file)
  await adminFetch(`/api/albums/${encodeURIComponent(albumId)}/photos`, { method: 'POST', body })
}, getAdminApiErrorMessage)
```

| 项 | 值 |
|---|---|
| method | `POST` |
| 路径 | `/api/albums/{albumId}/photos`，`albumId` 经 `encodeURIComponent` |
| Content-Type | **multipart/form-data**（传 `FormData` 时**不要**手设 Content-Type，让浏览器带 boundary） |
| 字段名 | **`files`** |
| 粒度 | **一次请求一个文件**（`enqueue([file], ...)`，`albums.vue:169`）。后端能处理多字段，但前端刻意单文件；后端注释确认：「the dashboard submits one request per file so successful files remain visible even when a later file is invalid.」 |
| 进度上报 | **无。** 整文件一次性 POST，无 `onUploadProgress`，`UploadItem` 无 progress 字段 |
| 取消 | **无。** 未传 `AbortSignal`；队列注释明确拒绝 abort（见 §0.2） |
| 响应体 | `Photo[]`（后端 `ApiResult<Json<Vec<Photo>>>`）；**前端丢弃返回值**，不解析 |

后端路由注册（`backend/src/main.rs:6894-6898`）：`/api/albums/{album_id}/photos` → `get(album_photos).post(upload_photos).layer(DefaultBodyLimit::disable())`（体积极限被显式关闭）。

### A.6.1 `adminFetch` 公共行为

`app/composables/useAdminApi.ts:102-126`：

- 必带 `headers.set('X-Requested-With', 'ChronoFrame')`（`:108`）。
- 非 GET/HEAD 时从 cookie `cf_csrf` 读值并设 `X-CSRF-Token`（`:110-113`；`readBrowserCookie` 在 `:41-57`）。
- `credentials: 'include'`（`:120`）。
- 收到 401 时 `markUnauthenticated()`（`:123`）。
- **先检查登录态**：未认证直接 `throw new Error('请先登录管理员账号')`（`:207-209`）；未 `checked` 或 `loading` 时先 `await refreshAuthStatus()`（`:203-205`，内部有 `pendingAuthStatusRequest` 单飞去重，`:39`、`:129-152`）。

### A.6.2 错误文案提取

`app/composables/useAdminApi.ts:65-76`：

```ts
export function getAdminApiErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (!error || typeof error !== 'object') return '请求失败，请稍后重试'

  const candidate = error as ApiErrorShape
  return (
    candidate.data?.error
    || candidate.data?.message
    || candidate.message
    || '请求失败，请稍后重试'
  )
}
```

优先级：`data.error` → `data.message` → `message` → 兜底。**React 里照抄这个顺序**，否则错误文案会和现网不一致。

### A.6.3 服务端校验分支（`backend/src/main.rs:4751+` `upload_photos`）

| 条件 | 状态码 | 文案 |
|---|---|---|
| 相册不存在 | 404 | `相簿不存在；请先创建相簿` |
| 扩展名与实际格式不符 | 400 | `{filename} 的扩展名与实际图片格式不一致` |
| 内容不可识别 | 400 | `{filename} 不是可识别的图片` |
| 扩展名不支持 | 400 | `{filename} 仅支持 PNG、JPG/JPEG、WEBP` |
| 内容格式不受支持（如 GIF） | 400 | `{filename} 的内容格式不受支持` |
| 空文件 | — | `bytes.is_empty()` → `continue`，静默跳过该字段 |
| 存储迁移/清理中 | 409 | `存储正在迁移或清理，请稍后再上传` |

## A.7 跨页保活：精确机制

### A.7.1 状态存在哪里（三条链路）

1. **`useState('admin-upload-queue', () => createUploadQueueState<File>())`**（`useAdminUploads.ts:7`）—— Nuxt 全局响应式状态，**不随组件卸载销毁**。
2. **`const controllers = new WeakMap<object, ReturnType<typeof createUploadQueue<File>>>()`**（`:3`）—— **模块级单例缓存**，key 是 `state.value` 对象本身（`:9-17`）：

   ```ts
   let controller = controllers.get(state.value)
   if (!controller) {
     controller = createUploadQueue(state.value, async (file, albumId) => { /* … */ }, getAdminApiErrorMessage)
     controllers.set(state.value, controller)
   }
   ```

   任何组件再次调用 `useAdminUploads()` 拿到的都是**同一个 controller 闭包**，闭包持有同一个 `state` 引用。
3. **在途的 `fetch` promise 与 `.finally(pump)` 链不属于任何组件** —— 它们由 `void` 启动（`admin-upload-queue.ts:42`），组件卸载不会取消它们。

此外 `open` 抽屉开关也在全局状态：`useState('admin-upload-queue-open', () => false)`（`:8`），**不是组件局部 ref**。所以 `albums.vue:198`、`layouts/dashboard.vue:42`、`tasks.vue:67` 都能从外部打开抽屉。

### A.7.2 三种导航的精确结果

| 用户动作 | 队列状态 | 在途请求 | 结果 |
|---|---|---|---|
| 后台内页面切换（`/dashboard/albums` → `/dashboard/downloads`） | 保留 | 继续 | **上传继续**（`layouts/dashboard.vue` 是持久布局；即使 `UploadQueue.vue` 被卸载，1+2+3 仍在）。UI 文案：「可切换后台页面，上传会继续」（`UploadQueue.vue:20`） |
| 离开 dashboard 布局（去前台 `/albums/...`、`/photos`） | 保留 | 继续 | **上传仍继续**：布局卸载，但 `useState` 全局状态与模块级 controller 仍在同一次 JS 运行时内，`File` 引用仍被 `item.file` 持有 |
| 刷新（F5）/ 硬导航 | **丢失** | 中断 | 整个 JS 运行时重建 → `useState` 工厂重新执行得到**空队列** |
| 关闭标签页 / 浏览器 | **丢失** | 中断 | 进程级销毁，全丢 |
| 退出登录 | 拦截 | — | `layouts/dashboard.vue:42`：`if (uploads.pending.value \|\| uploads.failed.value) { uploads.open.value = true; error.value = '请先处理上传队列，再退出登录。'; return }` |

### A.7.3 为什么「刷新 / 关浏览器 / 离开后台则不能保证继续」

- **刷新、关浏览器**：队列状态（含 `File` 句柄）只存在于内存。`File` 是**不可序列化的浏览器对象**，无法写入 `localStorage` / `sessionStorage` / Nuxt payload。**这是物理限制而非未实现的 TODO。** 重写时唯一正确的做法是保留警告，而不是假装能恢复。
- **提示语措辞**（`UploadQueue.vue:20`，`AAlert type="info" show-icon`）：
  - `message="可切换后台页面，上传会继续"`
  - `description="7 个并发任务，文件始终上传到选择时的相册。不要刷新或关闭浏览器；离线时队列不会自动重试。"`
- **`beforeunload` 警告**（`UploadQueue.vue:12-14`）：

  ```ts
  useEventListener('beforeunload', (event: BeforeUnloadEvent) => {
    if (pending.value || failed.value) { event.preventDefault(); event.returnValue = '' }
  })
  ```

  条件是 `pending`（uploading + queued）**或** `failed` 任一为真。注意：这只是浏览器原生确认框，**不保证**上传继续，也不阻止关闭。`event.returnValue = ''` 在现代浏览器已被忽略，但必须保留 `preventDefault()`；重写保持原样即可。
- `app/pages/dashboard/tasks.vue:67` 的卡片描述同样声明：「切换后台页面不影响上传，关闭浏览器会停止。」

## A.8 选择文件后的入队流程与 albumId 绑定

唯一入队调用（`app/pages/dashboard/albums.vue:167-171`）：

```ts
const queueFile = (file: File) => {
  if (!selectedAlbum.value || locked.value || !ready.value) return false
  if (!/\.(png|jpe?g|jepg|webp)$/i.test(file.name)) { notice.add({ title: `不支持此文件：${file.name}`, color: 'warning' }); return false }
  uploads.enqueue([file], { id: selectedAlbum.value.id, name: selectedAlbum.value.name })
  return false                                  // 返回 false 阻止 antd Upload 自行上传
}
```

- 入口控件：`AUpload`（`albums.vue:239`）与 `AUploadDragger`（`:265`、`:267`），均 `multiple :show-upload-list="false" :before-upload="queueFile"`。
- `accept` 常量（`albums.vue:47`）：`const accept = '.png,.jpg,.jpeg,.jepg,.webp'`。
  **⚠ `accept` 与正则里都有 `jepg` 拼写错误（`:47`、`:168`）。这是现网行为** —— 重写要显式决定保留还是修掉；修掉属于行为变更，建议保留并与后端确认。
- **`albumId` 在 `enqueue` 时快照**进每个 item（`admin-upload-queue.ts:55`）：

  ```ts
  for (const file of files) state.items.push({
    id: state.nextId++, albumId: album.id, albumName: album.name,
    name: file.name, size: file.size, file, status: 'queued', error: '',
  })
  ```

  此后**不改、不重新解析、不随 UI 选中相册变化**。所以用户选完 A 相册入队后切到 B 相册，A 的文件仍进 A。UI 提示：「文件始终上传到选择时的相册」（`UploadQueue.vue:20`）。
- **入队即 `pump()`**（`:58`），没有 debounce / 批量聚合。
- 失败时 `item.file` **保留**（只在成功时被清空），所以重试可行；也让「取消 / 移除」能真正释放引用。
- `enqueue` 不检查 `paused` —— 暂停期间项目照常入队，恢复后一起开始。

### A.8.1 成功计数触发图片列表刷新

`app/pages/dashboard/albums.vue:132-136`：

```ts
watch(() => uploads.state.value.albumVersions[selectedId.value], () => {
  // A continuous upload must not postpone visible results until the queue ends.
  if (uploadRefresh) return
  uploadRefresh = setTimeout(() => { uploadRefresh = undefined; if (selectedId.value) void loadDetail() }, 1000)
})
```

React 侧需要一个等价 effect 订阅 `albumVersions[albumId]`，**1 秒防抖**后重新拉取相册详情，否则上传成功后图片列表不刷新。

### A.8.2 上传期间的相册操作拦截

- `albums.vue:61`：`const albumUploads = computed(() => uploads.state.value.items.filter(item => item.albumId === selectedId.value && ['queued', 'uploading'].includes(item.status)).length)`
- `albums.vue:263`：`AAlert type="info"` `v-if="albumUploads"`，`message="\`${albumUploads} 张图片等待上传或处理中；可切换页面，成功入库后自动显示。\`"`，`#action` 内「查看队列」按钮。
- `albums.vue:198`（删除相册前）：`if (albumUploads.value) { uploads.open.value = true; notice.add({ title: '请先完成或取消此相册的上传队列', color: 'warning' }); return }`

## A.9 暂停 / 恢复语义

`shared/utils/admin-upload-queue.ts:60-61`：

```ts
pause() { state.paused = true },
resume() { state.paused = false; pump() },
```

- **`pause` 只设标志位，不触碰在途请求。** `pump` 首行 `if (state.paused) return`（`:34`）阻止**新任务启动**。
- **进行中的任务照常跑到 `done` / `failed`。** 其 `.finally(pump)` 虽会调用，但立即在首行返回，不会启动新任务。
- **恢复必须同时置 `false` 并调用 `pump()`。** 只置 `false` 而不 pump，队列会**永久卡住**（因为 `pump` 只在 `enqueue` / `resume` / `retryFailed` / 任务完成时被调用）。**React 重写极易漏掉这一点。**
- UI（`UploadQueue.vue:24`）：按钮仅在 `pending > 0` 时出现：

  ```vue
  <AButton v-if="pending" @click="state.paused ? queue.resume() : queue.pause()">{{ state.paused ? '继续上传' : '暂停队列' }}</AButton>
  ```
- 暂停提示（`UploadQueue.vue:28`）：

  ```vue
  <AAlert v-if="state.paused" type="warning" show-icon message="已暂停新任务，正在上传的文件会继续完成" class="mb-4" />
  ```

## A.10 失败重试、清除已完成记录、网络中断

### A.10.1 重试

`app/components/dashboard/UploadQueue.vue:9-11`：

```ts
const retry = async () => {
  if (await notice.confirm('失败请求可能已经入库，请先核对对应相册，避免重复上传。确认重新上传所有失败文件？')) queue.retryFailed()
}
```

`shared/utils/admin-upload-queue.ts:62-67`：

```ts
retryFailed() {
  for (const item of state.items) {
    if (item.status === 'failed' && item.file) { item.status = 'queued'; item.error = '' }
  }
  pump()
},
```

- 只重试**仍持有 `file`** 的失败项（`file` 只在成功时被清空，所以失败项通常总有 file）。
- **不记录重试次数**，无退避、无自动重试。
- 必弹确认（`notice.confirm` 默认非 danger），因为 `failed` 语义是「未确认成功」而非「确定失败」—— 可能已入库，重试会重复。
- 位置不变，因此会插到后续新入队项之前。
- 标签文案（`UploadQueue.vue:7-8`）：

  ```ts
  const labels = { queued: '等待上传', uploading: '上传与处理', done: '已入库', failed: '未确认成功' }
  const colors = { queued: 'default', uploading: 'processing', done: 'success', failed: 'error' }
  ```

  **重写必须沿用「未确认成功」而不是「失败」。**
- 按钮：`<AButton v-if="failed" @click="retry">重试失败文件</AButton>`（`:25`）。

### A.10.2 清除已完成记录

- 按钮（`UploadQueue.vue:26`）：`<AButton :disabled="!done" @click="queue.clearDone">清除已完成记录</AButton>`
- 实现（`admin-upload-queue.ts:72`）：`clearDone() { state.items = state.items.filter(item => item.status !== 'done') }`
  **注意是整体替换数组引用**，不是 `splice`。React 等价于 `setItems(prev => prev.filter(i => i.status !== 'done'))`。
- 文案（`UploadQueue.vue:36`）：「取消仅移除尚未开始的队列项；移除记录和清除记录均不会删除已入库的图片。」

### A.10.3 网络中断 / 离线

- **没有 `navigator.onLine` 监听、没有 `offline` / `online` 事件、没有自动重试、没有退避。**
- 提示语是静态的（`UploadQueue.vue:20` description 末句）：「**离线时队列不会自动重试。**」
- 离线上传会走 `adminFetch` → `$fetch` 抛错 → 被 `.catch` 分类为 `failed`，`error` 显示 `getAdminApiErrorMessage` 的结果（网络错误通常落到「请求失败，请稍后重试`」`）。**没有专门的「离线」分支。**
- 恢复网络后需要用户手动点「重试失败文件」并确认重复入库风险。

## A.11 UploadQueue.vue 完整 UI 规格（38 行全覆盖）

**触发按钮**（`:18`）：

```vue
<ABadge :dot="failed > 0"><AButton @click="open = true"><Icon name="tabler:cloud-upload" /> 上传队列<span v-if="pending">（{{ pending }}）</span></AButton></ABadge>
```

- `failed > 0` 时显示小红点；`pending > 0` 时按钮文案追加 `（N）`。

**抽屉**（`:19`）：

```vue
<ADrawer v-model:open="open" title="上传队列" width="min(720px, 100vw)" :destroy-on-close="false">
```

- **`destroy-on-close: false` 很重要** —— 关闭抽屉不销毁内容。但真正的保活来自 composable 而非此属性。
- `open` 在全局 `useState`（见 §A.7.1）。

**信息条**（`:20`）：`AAlert type="info" show-icon`，文案见 §A.7.3。

**统计行**（`:21`）：`<div class="admin-upload-summary">` 四个 span：

| 文案 | 值 | 渲染条件 |
|---|---|---|
| `已入库 <strong>{done}</strong>` | `done` | 始终 |
| `处理中 {active}` | `active` | 始终 |
| `待上传 {queued}` | `queued` | 始终 |
| `未确认 {failed}` | `failed` | **仅 `failed` 为真时**（`v-if="failed"`） |

**总进度条**（`:22`）：

```vue
<AProgress v-if="state.items.length" :percent="Math.round(done / state.items.length * 100)" :status="failed ? 'exception' : undefined" />
```

**分母是全部 item（含 failed），所以有失败时进度条永远到不了 100%** —— 这是有意的红条设计。

**操作区**（`:23-27`）：`<ASpace wrap class="mb-4">` 内含三个按钮：`暂停队列/继续上传`（`v-if="pending"`）、`重试失败文件`（`v-if="failed"`）、`清除已完成记录`（`:disabled="!done"`）。

**暂停警告**（`:28`）：见 §A.9。

**表格**（`:29`）：

```vue
<ATable :columns="columns" :data-source="state.items" row-key="id" size="small" :pagination="{ pageSize: 20, showSizeChanger: false }" :scroll="{ x: 480 }">
```

列定义（`:6`）：`{ title: '文件 / 相册', key: 'file' }`、`{ title: '状态', key: 'status', width: 160 }`、`{ title: '操作', key: 'action', width: 75 }`。

**`file` 单元格**（`:31`）：

```vue
<template v-if="column.key === 'file'"><div class="admin-file-name">{{ record.name }}</div><NuxtLink :to="{ path: '/dashboard/albums', query: { album: record.albumId } }" @click="open = false">{{ record.albumName }}</NuxtLink><span class="admin-help"> · {{ adminBytes(record.size) }}</span><div v-if="record.error" class="admin-field-error">{{ record.error }}</div></template>
```

- 文件名（`.admin-file-name`）→ 相册链接到 `/dashboard/albums?album={albumId}`，**点击时 `open = false`** 关闭抽屉 → ` · {adminBytes(size)}`（`.admin-help`）→ `record.error` 若有则 `.admin-field-error`。

**`status` 单元格**（`:32`）：`<ATag :color="colors[record.status]">{{ labels[record.status] }}</ATag>`

**`action` 单元格**（`:33`）：

```vue
<AButton v-if="column.key === 'action' && record.status !== 'uploading'" type="link" size="small" @click="queue.remove(record.id)">{{ record.status === 'queued' ? '取消' : '移除' }}</AButton>
```

- **`queued` 显示「取消」，其余（done / failed）显示「移除」**，均调用 `queue.remove(record.id)`。**没有二次确认。**

**底部说明**（`:36`）：`<p class="admin-help">取消仅移除尚未开始的队列项；移除记录和清除记录均不会删除已入库的图片。</p>`

**空状态**：**没有任何 `AEmpty` / `v-else` 分支。** 队列为空时表格显示 antd 默认 empty。

**`adminBytes`**（`app/utils/adminFormat.ts:1-6`）：

```ts
export function adminBytes(bytes: number) {
  if (!bytes || bytes < 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const index = Math.min(4, Math.floor(Math.log(bytes) / Math.log(1000)))
  return `${(bytes / 1000 ** index).toFixed(index ? 1 : 0)} ${units[index]}`
}
```

**用 1000 进制**，与 `settings/storage.vue:298-304` 里 1024 进制的 `formatBytes` **不是同一个函数，重写时不要合并**。

## A.12 其它使用点（React 需一并迁移）

| 位置 | 内容 |
|---|---|
| `app/layouts/dashboard.vue:80` | `<DashboardUploadQueue />` 常驻顶栏 |
| `app/layouts/dashboard.vue:13` | `const uploads = useAdminUploads()`（布局层，保证跨页存活） |
| `app/layouts/dashboard.vue:42` | 退出登录拦截（见 §A.7.2） |
| `app/pages/dashboard/tasks.vue:8` | `const uploads = useAdminUploads()` |
| `app/pages/dashboard/tasks.vue:67` | 卡片「本浏览器的上传队列」，`v-if="uploads.state.value.items.length"`，显示 `已入库 {done} · 上传中 {active} · 排队 {queued} · 未确认 {failed}`，描述「切换后台页面不影响上传，关闭浏览器会停止。」，按钮打开抽屉。**注意 `tasks.vue` 用 `uploads.done.value` 这种显式 `.value`**（因为在模板里通过对象访问 computed） |
| `app/pages/dashboard/albums.vue:12` | `const uploads = useAdminUploads()` |
| `app/pages/dashboard/albums.vue:61` | `albumUploads` computed |
| `app/pages/dashboard/albums.vue:132` | `albumVersions` 监听（1s 防抖刷新） |
| `app/pages/dashboard/albums.vue:169` | 唯一入队点 |
| `app/pages/dashboard/albums.vue:198` | 删除相册前拦截 |
| `app/pages/dashboard/albums.vue:263` | 待上传提示条 |

**React 建议**：`useAdminUploads` 等价物应实现为一个模块级 store（如 Zustand / 自建单例 + `useSyncExternalStore`），在 `AdminLayout` 挂载；**不要**放进任何会被卸载的页面组件。`shared/utils/admin-upload-queue.ts` 可**原样复制**为 TS 模块。

---

# B. 下载管理（DownloadManager + downloads.vue）

## B.1 页面壳与组件边界

`app/pages/dashboard/downloads.vue`（6 行）：

```vue
<script setup lang="ts">
definePageMeta({ layout: 'dashboard' })
useHead({ title: '下载管理' })
</script>

<template><DashboardDownloadManager /></template>
```

→ **下载 UI 100% 在 `app/components/dashboard/DownloadManager.vue`（226 行）内**，页面只是壳 + layout + title。组件被 Nuxt 自动导入（`nuxt.config.ts:20`：`components: [{ path: '~/components/ui', pathPrefix: false }, '~/components']`）。

## B.2 API 契约总表

前端调用点全部在 `DownloadManager.vue`；后端路由定义在 `backend/src/album_downloads.rs:199-223`（`Service::routes()`）。类型定义在 `shared/types/downloads.ts`（39 行）。

| # | method | 完整路径 | query | 请求体 | 响应体 | 前端行号 | 后端行号 |
|---|---|---|---|---|---|---|---|
| 1 | GET | `/api/album-downloads` | — | — | `AdminAlbumDownloads` | `:62` | `:246-276` |
| 2 | PUT | `/api/albums/{albumId}/download-settings` | — | 见 §B.2.1 | `{ queued: boolean }`（前端忽略） | `:80` | `:278-295` |
| 3 | PUT | `/api/album-downloads/settings/bulk` | — | 见 §B.2.2 | `{ updated: number, queued: boolean }`（前端只取 `updated`） | `:114` | `:297-308` |
| 4 | POST | `/api/albums/{albumId}/downloads/rebuild` | — | 无 body | 未解析 | `:126` | `:211` |
| 5 | POST | `/api/album-downloads/{jobId}/cancel` | — | 无 body | 未解析 | `:136` | `:221` |
| 6 | DELETE | `/api/album-downloads/{jobId}` | — | 无 body | 未解析 | `:136` | `:222` |
| 7 | GET | `/api/albums/{albumId}/downloads/{format}?version={jobId}` | `version` | — | ZIP 字节流 | `:199`（`AButton :href`，**原生导航非 fetch**） | `:212` |

> `albumId` 取自 `selected`（见 §B.4），**前端未做 `encodeURIComponent`**（与上传路径不同，注意别在重写时「顺手」改）。

### B.2.1 单相册保存请求体（`DownloadManager.vue:80`）

```ts
await adminFetch(`/api/albums/${selected.value}/download-settings`, {
  method: 'PUT',
  body: { enabled: draft.enabled, formats: draft.formats, maxImageBytes: Math.round((draft.imageMB || 0) * 1_000_000), maxZipBytes: 0 },
})
```

### B.2.2 批量保存请求体（`DownloadManager.vue:108-114`）

```ts
const settings = { enabled: bulkDraft.enabled, formats: [...bulkDraft.formats], maxImageBytes: Math.round((bulkDraft.imageMB || 0) * 1_000_000), maxZipBytes: 0 }
const target = bulkScope.value === 'all' ? { scope: 'all' } : { scope: 'selected', albumIds: [...bulkSelected.value] }
const result = await adminFetch<{ updated: number }>('/api/album-downloads/settings/bulk', { method: 'PUT', body: { target, settings } })
```

### B.2.3 后端反序列化契约（必须逐字匹配）

`backend/src/album_downloads.rs:143-168`：

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsInput {
    enabled: bool,
    formats: Vec<String>,
    max_image_bytes: i64,
    #[serde(default)]
    max_zip_bytes: i64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "scope", rename_all = "camelCase", deny_unknown_fields)]
enum SettingsTarget {
    Selected { #[serde(rename = "albumIds")] album_ids: Vec<String> },
    All {},
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BulkSettingsInput { target: SettingsTarget, settings: SettingsInput }
```

**⚠ 关键陷阱**：`deny_unknown_fields` + internally-tagged enum 意味着 `{"scope":"all","albumIds":[...]}` **会被拒绝**（有测试 `bulk_targets_reject_ambiguous_scopes`，`:1617-1621`，断言 `{"scope":"all","albumIds":["a"]}`、`{"scope":"selected"}`、`{"scope":"unknown"}` 全部报错）。传 `{scope:'all'}` 时**绝不能带 `albumIds` 键**。

`max_zip_bytes` 有 `#[serde(default)]`，但前端始终显式传 `0`（= 不限）。

### B.2.4 后端校验（`validate`，`:170-197`）

| 规则 | 违反时文案 |
|---|---|
| `formats` 非空且 ≤ 4 | `请选择至少一种格式` |
| 值 ∈ {png, jpg, jpeg, webp}（转小写后判定） | `仅支持 PNG、JPG、JPEG 和 WebP` |
| `max_image_bytes == 0` 或 ∈ [16_384, 500_000_000] | `单张图片上限须为 16 KB 至 500 MB，0 表示不限` |
| `max_zip_bytes == 0` 或 ∈ [1_000_000, 1_000_000_000_000] | `ZIP 上限须为 1 MB 至 1 TB，0 表示不限` |

`formats` 会被 `to_lowercase()` → `sort()` → `dedup()`（`:174-187`）。

**⚠ 现网存在的可触发路径**：前端 `AInputNumber :min="0" :max="500" :step="0.5"`（MB）→ `×1e6`。用户若输入 `0.001` MB = 1000 字节，前端放行但后端拒绝（< 16384）。**前端没有做下限校验。** 重写时建议保持一致或明确改进。

### B.2.5 列表响应体形状（后端 `admin_list`，`:246-276`）

`require_admin(&headers, &state, false)` —— 注意 `false` = 读接口不强制 CSRF。

```json
{
  "settings": [{
    "albumId": "…",
    "albumName": "…",
    "enabled": false,
    "formats": ["webp"],
    "maxImageBytes": 5000000,
    "maxZipBytes": 0,
    "revision": 0
  }],
  "jobs": [{
    "id": "…",
    "albumId": "…",
    "albumName": "…",
    "format": "webp",
    "revision": 0,
    "status": "queued",
    "total": 0,
    "completed": 0,
    "byteSize": 0,
    "error": null,
    "createdAt": 0,
    "updatedAt": 0
  }],
  "localBytes": 0,
  "directory": "data/album-downloads"
}
```

后端构造细节：

- `settings` 来自 `SELECT a.id,a.name,s.* FROM albums a LEFT JOIN album_download_settings s ON s.album_id=a.id ORDER BY a.position, a.created_at DESC`（`:251`）。
  **无设置行的相册也会返回**，默认值硬编码为 `enabled: false`、`formats: ["webp"]`、`maxImageBytes: 5_000_000`、`maxZipBytes: 0`、`revision: 0`（`:253-260`）。
- `jobs` 来自 `SELECT j.*,a.name album_name FROM album_download_jobs j JOIN albums a ON a.id=j.album_id ORDER BY j.created_at DESC LIMIT 500`（`:252`）。**上限 500 条，按创建时间倒序。**
- `localBytes` = `SELECT COALESCE(SUM(byte_size),0) FROM album_download_jobs WHERE status='ready'`（`:267-272`）。
- `directory` = 硬编码字符串 `"data/album-downloads"`（`:274`），**前端直接展示**（`:205`）。

TS 类型（`shared/types/downloads.ts:34-39`）：

```ts
export interface AdminAlbumDownloads {
  settings: AlbumDownloadSettings[]
  jobs: AlbumDownloadJob[]
  localBytes: number
  directory: string
}
```

### B.2.6 `revision` 语义（关键，不可丢）

后端建 5 个 SQLite 触发器，在图片增、删、改（album_id / storage_key / original_name / format）与相册改名时 `revision = revision + 1`（`backend/src/album_downloads.rs:127-138`）：

```sql
CREATE TRIGGER IF NOT EXISTS downloads_photo_insert AFTER INSERT ON photos BEGIN
  UPDATE album_download_settings SET revision=revision+1,updated_at=unixepoch() WHERE album_id=NEW.album_id;
END;
CREATE TRIGGER IF NOT EXISTS downloads_photo_delete AFTER DELETE ON photos BEGIN
  UPDATE album_download_settings SET revision=revision+1,updated_at=unixepoch() WHERE album_id=OLD.album_id;
END;
CREATE TRIGGER IF NOT EXISTS downloads_photo_update AFTER UPDATE OF album_id,storage_key,original_name,format ON photos BEGIN
  UPDATE album_download_settings SET revision=revision+1,updated_at=unixepoch() WHERE album_id IN (OLD.album_id,NEW.album_id);
END;
CREATE TRIGGER IF NOT EXISTS downloads_album_rename AFTER UPDATE OF name ON albums WHEN NEW.name<>OLD.name BEGIN
  UPDATE album_download_settings SET revision=revision+1,updated_at=unixepoch() WHERE album_id=NEW.id;
END;
```

前端把 `revision` 当作「当前版本」，用于 jobs 过滤（`:33`）与状态判定（`:40`）。

**重写时必须保留「只有 `job.revision === settings.revision` 的 job 才算当前版本」这条规则**，否则历史 ZIP 会混进当前状态。

`album_download_jobs` 表有 `UNIQUE(album_id, format, revision)`（`:125`）—— 所以每个相册 / 格式 / 版本组合只有一个 job 行。

## B.3 每个相册的下载设置字段

TS 类型（`shared/types/downloads.ts:11-19`）：

```ts
export type DownloadFormat = 'png' | 'jpg' | 'jpeg' | 'webp'

export interface AlbumDownloadSettings {
  albumId: string
  albumName: string
  enabled: boolean
  formats: DownloadFormat[]
  maxImageBytes: number
  maxZipBytes: number
  revision: number
}
```

表单草稿 `draft`（`DownloadManager.vue:15`）：

```ts
const draft = reactive({ enabled: false, formats: ['webp'] as DownloadFormat[], imageMB: 5 })
```

| 字段 | 控件 | 行号 | 说明文案（extra） |
|---|---|---|---|
| `enabled` | `ASwitch checked-children="开启" un-checked-children="关闭"` | `:181` | 「开启后，系统自动生成所选格式的压缩包。」 |
| `formats` | `ACheckboxGroup`，选项 PNG / JPG / JPEG / WEBP | `:182`（选项 `:47`） | 「每种格式生成一个独立 ZIP。JPG 与 JPEG 编码相同，扩展名不同。」 |
| `imageMB` | `AInputNumber :min="0" :max="500" :step="0.5"` | `:183` | 「0 表示不限。必要时降低画质或分辨率；PNG 保持无损编码，通过缩小尺寸达标。」 |
| `maxZipBytes` | **无 UI，恒发 `0`** | `:80` | — |

格式选项值（`:47`）：

```ts
const formats = ['png', 'jpg', 'jpeg', 'webp'].map(value => ({ label: value.toUpperCase(), value }))
```

### B.3.1 `apply()` 与 dirty 判定

`DownloadManager.vue:30-31, 52-56`：

```ts
const signature = computed(() => JSON.stringify(draft))
const dirty = computed(() => !!saved.value && signature.value !== saved.value)

const apply = (config?: AlbumDownloadSettings) => {
  if (!config) return
  Object.assign(draft, { enabled: config.enabled, formats: [...config.formats], imageMB: config.maxImageBytes / 1_000_000 })
  saved.value = signature.value
}
```

- **`formats` 必须浅拷贝**（`[...config.formats]`），否则会和 `data.settings` 里的数组共享引用，导致 dirty 计算永远相等。
- **`saved` 初始为 `''`，所以未加载前 `dirty` 恒为 `false`**（`!!saved.value` 守卫）。
- 表单（`:180-186`）：

  ```vue
  <AForm name="album-download-settings" layout="vertical" :model="draft" :disabled="saving" @finish="save">
  …
  <ASpace><AButton type="primary" html-type="submit" :loading="saving" :disabled="!dirty">保存下载设置</AButton><AButton :disabled="!dirty || saving" @click="apply(current)">放弃修改</AButton></ASpace>
  <p v-if="dirty" class="admin-help mt-3">有未保存的修改</p>
  ```

  **提交靠 `html-type="submit"` + `@finish`，不是 `@click`。**

### B.3.2 `save()` 完整分支（`:74-86`）

```ts
const save = async () => {
  if (!current.value || saving.value) return
  if (!draft.formats.length) { notice.add({ title: '请选择至少一种图片格式', color: 'warning' }); return }
  saving.value = true
  try {
    if (!draft.enabled && current.value.enabled && !await notice.confirm('关闭公开下载并删除该相册已生成的本地 ZIP？原始图片不受影响。', true)) return
    await adminFetch(`/api/albums/${selected.value}/download-settings`, { method: 'PUT', body: { enabled: draft.enabled, formats: draft.formats, maxImageBytes: Math.round((draft.imageMB || 0) * 1_000_000), maxZipBytes: 0 } })
    saved.value = signature.value
    await load(true)
    notice.add({ title: draft.enabled ? '已保存，系统将在后台生成压缩包' : '已关闭公开下载，本地压缩包将自动清理', color: 'success' })
  } catch (cause) { notice.add({ title: '保存失败', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { saving.value = false }
}
```

分支顺序：

1. `!current` 或 `saving` → 直接返回。
2. `formats` 为空 → warning，**不发请求**。
3. `saving = true`。
4. **`enabled` 由 true → false 的额外确认**（`notice.confirm(..., true)`，第二参数 `true` = danger 样式）：

   > `关闭公开下载并删除该相册已生成的本地 ZIP？原始图片不受影响。`

   这个 check 在 `saving = true` **之后**、请求**之前**（`try` 块内）；取消则 `finally` 会重置 `saving`。
5. 发请求 → `saved.value = signature.value` → `await load(true)` → 成功通知。
6. `catch` → 「保存失败」error 通知。
7. `finally` → `saving = false`。

## B.4 独立页的相册选择上下文

`DownloadManager.vue:11`：

```ts
const selected = computed(() => props.embedded ? (props.albumId || '') : (typeof route.query.album === 'string' ? route.query.album : ''))
```

- **非 embedded 时来源是 `route.query.album`** —— 即 **URL 是唯一真相**。
- `pick()`（`:71-73`）：

  ```ts
  const pick = async (value: unknown) => {
    await router.push({ path: '/dashboard/downloads', query: value ? { album: String(value) } : {} })
  }
  ```

  **只改 URL，不回写任何本地状态。** React 重写应改为 searchParams / 路由参数驱动，**不要用 useState 镜像路由**。
- `current`（`:32`）：`data.settings.find(item => item.albumId === selected.value)`。
- 列表 / 详情切换由 `v-if="!embedded && !selected"`（`:163`）与 `v-if="!embedded && current"`（`:175`）控制。

### B.4.1 相册列表 UI（独立页、未选中相册时）

**页头**（`:160`）：

```vue
<DashboardPageHeader v-if="!embedded" :title="current ? `${current.albumName} · 下载设置` : '下载管理'" description="集中查看公开下载状态；批量设置会覆盖选中相册的单独设置。"><AButton v-if="selected" @click="pick('')">返回下载列表</AButton><AButton :loading="loading" @click="load()">刷新</AButton></DashboardPageHeader>
```

**工具栏**（`:164`）：

```vue
<div class="admin-toolbar"><ASpace wrap><AInputSearch v-model:value="search" placeholder="搜索相册" aria-label="搜索下载相册" allow-clear style="width:240px" /><AButton type="primary" :disabled="!data.settings.length" @click="openBulk">{{ checkedAlbums.length ? `批量设置（${checkedAlbums.length}）` : '批量设置' }}</AButton><AButton v-if="checkedAlbums.length" @click="checkedAlbums = []">取消选择</AButton></ASpace><AStatistic title="本地 ZIP 占用" :value="adminBytes(data.localBytes)" :value-style="{ fontSize: 20 }" /></div>
```

**搜索过滤**（`:12`, `:34`）：

```ts
const search = ref('')
const filteredSettings = computed(() => data.value.settings.filter(item => item.albumName.toLocaleLowerCase().includes(search.value.trim().toLocaleLowerCase())))
```

**表格**（`:165`）：

```vue
<ATable :columns="albumColumns" :data-source="filteredSettings" row-key="albumId" :row-selection="{ selectedRowKeys: checkedAlbums, onChange: (keys: (string | number)[]) => checkedAlbums = keys.map(String), preserveSelectedRowKeys: true }" :pagination="{ pageSize: 20, showSizeChanger: false }" :scroll="{ x: 750 }">
```

- `checkedAlbums` 是 `ref<string[]>([])`（`:13`）。**`preserveSelectedRowKeys: true` 保证翻页不丢选择。**

**列定义**（`:35`）：`相册`（dataIndex `albumName`）、`公开下载`（key `enabled`, 130）、`格式`（key `formats`, 220）、`当前任务`（key `status`, 130）、`操作`（key `actions`, 110）。

**单元格**（`:167-172`）：

- `albumName`：`<AButton type="link" class="admin-name-link" @click="pick(record.albumId)">{{ record.albumName }}</AButton>`
- `enabled`：`<ATag :color="record.enabled ? 'green' : 'default'">{{ record.enabled ? '已开启' : '未开启' }}</ATag>`
- `formats`：`<ATag v-for="format in record.formats" :key="format">{{ format.toUpperCase() }}</ATag>`
- `status`：`<span>{{ albumStatus(record.albumId) }}</span>`
- `actions`：`<AButton type="link" @click="pick(record.albumId)">管理下载</AButton>`

### B.4.2 详情页工具栏（已选中相册）

`:175`：

```vue
<div v-if="!embedded && current" class="admin-toolbar"><ASpace><span>切换相册</span><ASelect :value="selected" :options="options" :disabled="saving" show-search option-filter-prop="label" aria-label="管理相册" style="width:260px;max-width:65vw" @change="pick" /><AButton @click="openBulk">批量设置</AButton></ASpace><NuxtLink :to="{ path: '/dashboard/albums', query: { album: selected } }">管理此相册图片 →</NuxtLink></div>
```

`options`（`:46`）：`data.settings.map(item => ({ value: item.albumId, label: item.albumName }))`。

## B.5 `albumStatus()` 状态推导（`:36-45`）

```ts
const albumStatus = (albumId: string) => {
  const config = data.value.settings.find(item => item.albumId === albumId)
  if (!config) return '—'
  if (!config.enabled) return '未开启'
  const currentJobs = data.value.jobs.filter(job => job.albumId === config.albumId && job.revision === config.revision)
  if (currentJobs.some(job => ['failed', 'interrupted'].includes(job.status))) return '需处理'
  if (currentJobs.some(job => ['running', 'queued', 'deleting'].includes(job.status))) return '进行中'
  if (currentJobs.length && currentJobs.every(job => job.status === 'ready')) return '可下载'
  return '未就绪'
}
```

**优先级严格自上而下：需处理 > 进行中 > 可下载 > 未就绪。** 注意 `every` 前先判 `currentJobs.length`，空数组会落到「未就绪」。React 里抽成纯函数即可。

## B.6 批量设置语义（「勾选多个」vs「覆盖全部」）

### B.6.1 打开（`openBulk`，`:87-95`）

```ts
const openBulk = () => {
  if (!data.value.settings.length || saving.value) return
  if (dirty.value) { notice.add({ title: '请先保存或重置当前相册的修改', color: 'warning' }); return }
  bulkScope.value = 'selected'
  bulkSelected.value = checkedAlbums.value.length ? [...checkedAlbums.value] : selected.value ? [selected.value] : []
  Object.assign(bulkDraft, { enabled: current.value?.enabled ?? true, formats: [...(current.value?.formats || ['webp'])], imageMB: (current.value?.maxImageBytes ?? 5_000_000) / 1_000_000 })
  bulkBaseline.value = bulkSignature.value
  bulkOpen.value = true
}
```

- 无相册或正在保存 → 返回；有未保存修改 → warning 拦截。
- **每次打开都把 `bulkScope` 重置为 `'selected'`。**
- 预选优先级：**列表勾选 > 当前相册 > 空**。
- 默认值来自「当前相册」；无当前相册时 `enabled: true, formats: ['webp'], imageMB: 5`（5_000_000 字节，与后端默认一致）。

### B.6.2 脏检查（`:25-27`）

```ts
const bulkSignature = computed(() => JSON.stringify([bulkScope.value, bulkSelected.value, bulkDraft]))
const bulkDirty = computed(() => bulkOpen.value && bulkBaseline.value !== bulkSignature.value)
const bulkCount = computed(() => bulkScope.value === 'all' ? data.value.settings.length : bulkSelected.value.length)
```

**`bulkSignature` 把 `scope`、`selected`、`draft` 三者都算进去** —— 所以只切换「应用范围」单选也会被判定为脏，关闭时会弹确认。

### B.6.3 关闭（`closeBulk`，`:96-100`）

```ts
const closeBulk = async () => {
  if (bulkSaving.value) return
  if (bulkDirty.value && !await notice.confirm('批量设置尚未应用，确定放弃修改吗？')) return
  bulkOpen.value = false
}
```

### B.6.4 两种范围的差别（本项核心答案）

| 维度 | 勾选多个相册（`selected`） | 覆盖全部现有相册（`all`） |
|---|---|---|
| 单选值 `bulkScope` | `'selected'` | `'all'` |
| 请求体 `target` | `{ scope: 'selected', albumIds: [...bulkSelected.value] }` | `{ scope: 'all' }`（**无 `albumIds` 键**） |
| 相册选择器 | 显示（`v-if="bulkScope === 'selected'"`，`:215`） | 隐藏 |
| 计数 `bulkCount` | `bulkSelected.value.length` | `data.value.settings.length` |
| **端点** | `PUT /api/album-downloads/settings/bulk` | **同一个端点** |

**所以两者调用的 API 完全相同，只是 `target` 判别联合不同。** 后端 `apply_settings(db, target, input)`（`:310`）内部解析 target 得到 album id 列表，返回 `updated` 数量。

**原子性保证**（`:316-319` 注释）：

```rust
// Acquire the write lock before resolving targets. The worker and other admins
// must never observe a partially applied batch or race a read-to-write upgrade.
let mut tx = db.begin_with("BEGIN IMMEDIATE")
```

**语义 = 全量覆盖，不是合并**（后端直接替换 enabled / formats / maxImageBytes，并递增 `revision`）。UI 明确声明（`:210`）：

```vue
<AAlert type="warning" show-icon message="统一覆盖各相册的单独设置" description="下载开关、图片格式和大小上限都会替换为下方的设置，不会合并。只影响本次指定的现有相册，之后新建的相册不受影响。" class="mb-5" />
```

### B.6.5 保存校验与确认弹窗（`saveBulk`，`:101-120`）

**校验顺序**（全部 warning 通知，均不发请求）：

1. `if (bulkSaving.value) return`
2. `if (!bulkCount.value)` → `请至少选择一个相册`
3. `if (!bulkDraft.formats.length)` → `请选择至少一种图片格式`
4. `if (bulkScope.value === 'selected' && bulkSelected.value.some(id => !data.value.settings.some(item => item.albumId === id)))` → `选中的相册已不存在，请重新选择`
   （**防止弹窗期间相册被删除**）

**确认弹窗**（`:113`，`notice.confirm(..., true)` = danger，动态插值）：

```
将覆盖${scope}的下载开关、图片格式和大小上限：${settings.enabled ? '开启下载' : '关闭下载'}，${settings.formats.join(' / ').toUpperCase()}，单张${settings.maxImageBytes ? `最多 ${bulkDraft.imageMB} MB` : '不限大小'}。旧 ZIP 将清理${settings.enabled ? '并在后台重新生成' : ''}，原始图片不受影响。确定应用？
```

其中 `scope`（`:110`）：

```ts
const scope = bulkScope.value === 'all' ? `全部现有相册（当前 ${bulkCount.value} 个）` : `选中的 ${bulkCount.value} 个相册`
```

**成功**（`:115-117`）：

```ts
await adminFetch<{ updated: number }>('/api/album-downloads/settings/bulk', { method: 'PUT', body: { target, settings } })
bulkOpen.value = false
await load(true)
notice.add({ title: `已统一设置 ${result.updated} 个相册`, description: settings.enabled ? '压缩包将在后台自动更新，可以离开页面。' : '已关闭公开下载，本地 ZIP 将自动清理；原始图片不受影响。', color: 'success' })
```

**失败**（`:118`）：「批量设置失败」+ `getAdminApiErrorMessage(cause)`。
**`finally`**：`bulkSaving.value = false`。

### B.6.6 批量模态框 UI（`:209-224`）

```vue
<AModal :open="bulkOpen" title="批量设置相册下载" :width="640" :footer="null" :closable="!bulkSaving" :mask-closable="!bulkSaving" :keyboard="!bulkSaving" @cancel="closeBulk">
```

**保存中禁用所有关闭途径**（`closable` / `maskClosable` / `keyboard` 全部由 `bulkSaving` 门控），且 `:footer="null"`（按钮在表单内）。

内部结构：

1. `AAlert type="warning"` —— 见 §B.6.4。
2. `AForm name="bulk-download-settings" layout="vertical" :model="bulkDraft" :disabled="bulkSaving" @finish="saveBulk"`（`:211`）。
3. **应用范围**（`:212-214`）：

   ```vue
   <AFormItem label="应用范围"><ARadioGroup v-model:value="bulkScope" aria-label="应用范围" :options="[{ label: '选定相册', value: 'selected' }, { label: `全部相册（${data.settings.length}）`, value: 'all' }]" /></AFormItem>
   ```
4. **选择相册**（`:215-217`，仅 `scope === 'selected'`）：

   ```vue
   <AFormItem v-if="bulkScope === 'selected'" label="选择相册" required><ASelect v-model:value="bulkSelected" mode="multiple" :options="options" show-search option-filter-prop="label" aria-label="选择相册" allow-clear placeholder="搜索并选择一个或多个相册" :max-tag-count="4" /></AFormItem>
   ```
5. **计数提示**（`:218`）：`<p class="admin-help mb-5">将覆盖 {{ bulkCount }} 个相册。请确认下方设置后应用。</p>`
6. **可供下载**（`:219`）：`ASwitch`，extra「开启后自动生成 ZIP；关闭后撤下下载并清理本地 ZIP，原始图片不受影响。」
7. **图片格式**（`:220`）：`ACheckboxGroup`，extra「每种格式生成一个独立 ZIP。」
8. **单张图片大小上限（MB）**（`:221`）：`AInputNumber :min="0" :max="500" :step="0.5"`，extra「0 表示不限。必要时降低画质或缩小尺寸，**不会跳过超限图片**。」
9. **按钮**（`:222`）：

   ```vue
   <div style="display:flex;justify-content:flex-end;gap:8px"><AButton :disabled="bulkSaving" @click="closeBulk">取消</AButton><AButton type="primary" html-type="submit" :loading="bulkSaving" :disabled="!bulkCount || !bulkDraft.formats.length">覆盖并应用（{{ bulkCount }}）</AButton></div>
   ```

## B.7 任务列表（本地压缩包卡片）

### B.7.1 卡片结构（`:188-206`）

```vue
<ACard title="本地压缩包" :body-style="{ padding: '16px' }">
  <template #extra><AButton :disabled="!current.enabled" :loading="saving" @click="rebuild">重新生成</AButton></template>
  <p class="admin-help">生成任务在服务器后台运行，离开页面不受影响。增删图片、改名后自动更新；ZIP 仅存本机，不占用 S3 / WebDAV。</p>
  <div class="admin-toolbar"><span>{{ history ? '全部版本（含历史记录）' : '当前版本' }}</span><ACheckbox v-model:checked="history">显示历史记录</ACheckbox></div>
  <ATable :columns="columns" :data-source="jobs" row-key="id" size="middle" :pagination="{ pageSize: 8, showSizeChanger: false }" :scroll="{ x: 700 }">
```

- 卡头按钮「重新生成」：`:disabled="!current.enabled"`，`:loading="saving"`。
- 底部说明（`:205`）：「存放位置：{{ data.directory }}。删除后当前版本不会自动重建，点击"重新生成"即可恢复。」

### B.7.2 当前版本 vs 历史记录

`DownloadManager.vue:14, 33`：

```ts
const history = ref(false)
const jobs = computed(() => data.value.jobs.filter(job => job.albumId === selected.value && (history.value || job.revision === current.value?.revision)))
```

- `history = false`（默认）→ 只显示 `revision === current.revision` 的 jobs，即**当前 ZIP 版本**。
- `history = true` → 显示该相册**全部版本**（受后端 `LIMIT 500` 限制）。
- 左侧文案随 `history` 切换：「当前版本」/「全部版本（含历史记录）」。
- **ZIP 版本的双重标识**：`format` + `revision`。表格里同时展示（`:194`）：

  ```vue
  <template v-if="column.dataIndex === 'format'"><strong>{{ record.format.toUpperCase() }}</strong><div class="admin-help">v{{ record.revision }}</div></template>
  ```

- 下载 URL 用 **job id** 而非 revision 定位具体产物：`/api/albums/{albumId}/downloads/{format}?version={job.id}`（`:199`）。

### B.7.3 表格列与单元格

列定义（`:48`）：`格式`（dataIndex `format`, 80）、`状态 / 进度`（key `status`, 180）、`文件大小`（key `size`, 105）、`生成时间`（key `created`, 155）、`操作`（key `actions`, 160）。

**状态 / 进度**（`:195`）：

```vue
<template v-else-if="column.key === 'status'"><ATag :color="downloadStatusColor[record.status]">{{ downloadStatus[record.status] || record.status }}</ATag><AProgress v-if="record.status === 'running'" :percent="record.total ? Math.round(record.completed / record.total * 100) : 0" size="small" /><div v-if="record.error" class="admin-help" style="color:#cf1322">{{ record.error }}</div></template>
```

- `status === 'running'` 才显示进度条；`total === 0` 时为 0%。
- `record.error` 用硬编码红色 `#cf1322`。

**文件大小**（`:196`）：`{{ record.byteSize ? adminBytes(record.byteSize) : '—' }}`

**生成时间**（`:197`）：`{{ new Date(record.createdAt * 1000).toLocaleString('zh-CN', { hour12: false }) }}`（**后端是秒，前端 ×1000**）

**操作**（`:198-202`）：

```vue
<template v-else-if="column.key === 'actions'"><ASpace>
  <AButton v-if="record.status === 'ready' && record.revision === current.revision && current.enabled" type="link" size="small" :href="`/api/albums/${record.albumId}/downloads/${record.format}?version=${record.id}`">下载</AButton>
  <AButton v-if="['queued','running'].includes(record.status)" type="link" size="small" :loading="actionId === record.id" @click="jobAction(record.id, 'cancel')">取消</AButton>
  <APopconfirm v-if="!['deleted','deleting'].includes(record.status)" title="删除本地压缩包？原始图片不会删除，可随时重新生成。" ok-text="删除" :ok-button-props="{ danger: true }" @confirm="jobAction(record.id, 'delete')"><AButton type="link" danger size="small" :loading="actionId === record.id">删除</AButton></APopconfirm>
</ASpace></template>
```

- **下载按钮三条件**：`status === 'ready'` **且** `revision === current.revision` **且** `current.enabled`。是原生 `<a href>` 导航，不经过 `adminFetch`。
- **取消**：仅 `queued` / `running`，**无二次确认**。
- **删除**：`Popconfirm` 确认，`v-if="!['deleted','deleting'].includes(record.status)"`。

### B.7.4 状态映射（`app/utils/adminFormat.ts:7-8`）

```ts
export const downloadStatus: Record<string, string> = { queued: '排队中', running: '正在打包', ready: '可下载', failed: '生成失败', cancelled: '已取消', deleting: '正在删除', deleted: '已删除' }
export const downloadStatusColor: Record<string, string> = { queued: 'default', running: 'processing', ready: 'success', failed: 'error', cancelled: 'warning', deleting: 'processing', deleted: 'default' }
```

**这两个映射在 `app/utils/adminFormat.ts`，不在组件内，迁移时容易漏。** 未匹配的状态回退为**原始英文 status 字符串**（`:195` 的 `|| record.status`）。

### B.7.5 空状态

- 任务表为空：`ATable` 默认 empty，**无自定义空状态**。
- 组装为空时的三条提示：
  - `:176`：`<AAlert v-if="selected && !current && !loading && !error" type="warning" message="此相册不存在，请返回列表重新选择。" />`
  - `:177`：`<AAlert v-if="!data.settings.length && !loading" type="info" show-icon message="请先创建相册，再配置公开下载。" />`
  - `:162`：`<AAlert v-if="error" type="error" show-icon :message="error" />`
- 列表表格为空：antd 默认 empty。

## B.8 任务操作

### B.8.1 `jobAction()`（`:132-141`）

```ts
const jobAction = async (id: string, action: 'cancel' | 'delete') => {
  if (actionId.value) return
  actionId.value = id
  try {
    await adminFetch(`/api/album-downloads/${id}${action === 'cancel' ? '/cancel' : ''}`, { method: action === 'cancel' ? 'POST' : 'DELETE' })
    await load()
    notice.add({ title: action === 'cancel' ? '已请求取消任务' : '已撤下下载，正在删除本地 ZIP', color: 'success' })
  } catch (cause) { notice.add({ title: '操作失败', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { actionId.value = '' }
}
```

- URL 拼接：cancel → `/api/album-downloads/{id}/cancel`（POST）；delete → `/api/album-downloads/{id}`（DELETE）。
- `actionId` 是**单个字符串**（`:28`），所以**同时只能有一个任务操作在途**，所有行共享 `:loading="actionId === record.id"`。
- **删除有 Popconfirm 二次确认；取消没有。**
- 操作后 `await load()`（**不是 `load(true)`**），所以不会重置表单草稿。

### B.8.2 `rebuild()`（`:121-131`）

```ts
const rebuild = async () => {
  if (!current.value?.enabled || saving.value) return
  if (dirty.value) { notice.add({ title: '请先保存或放弃下载设置', color: 'warning' }); return }
  saving.value = true
  try {
    await adminFetch(`/api/albums/${selected.value}/downloads/rebuild`, { method: 'POST' })
    await load()
    notice.add({ title: '已提交重新生成任务，可以离开页面', color: 'success' })
  } catch (cause) { notice.add({ title: '提交失败', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { saving.value = false }
}
```

- 三处共用 `saving` 锁：**保存设置**、**重新生成**（`:124` 的 `saving` 也是表单 `:disabled="saving"` 的来源）。
- `:disabled="!current.enabled"`（`:189`）已挡未开启，函数内 `!current.value?.enabled` 是二重保险。
- `dirty` 时拒绝，必须先保存或放弃。

## B.9 进度轮询（端点 / 间隔 / 停止条件）

`DownloadManager.vue:49-51, 57-70, 142-144`：

```ts
let timer: ReturnType<typeof setTimeout> | undefined
let mounted = false
let loadRequest: Promise<void> | undefined

const load = async (reset = false): Promise<void> => {
  if (loadRequest) { await loadRequest; if (reset) return load(true); return }
  loading.value = true
  loadRequest = (async () => {
    try {
      data.value = await adminFetch<AdminAlbumDownloads>('/api/album-downloads')
      error.value = ''
      checkedAlbums.value = checkedAlbums.value.filter(id => data.value.settings.some(item => item.albumId === id))
      if (reset || !saved.value) apply(current.value)
    } catch (cause) { error.value = getAdminApiErrorMessage(cause) }
    finally { loading.value = false; loadRequest = undefined }
  })()
  await loadRequest
}

const poll = async () => { await load(); if (mounted) timer = setTimeout(poll, document.hidden ? 15000 : 3000) }
onMounted(() => { mounted = true; void poll() })
onBeforeUnmount(() => { mounted = false; clearTimeout(timer) })
```

| 项 | 值 |
|---|---|
| **端点** | `GET /api/album-downloads`（每次**全量重取** settings + 最多 500 条 jobs + `localBytes`） |
| **首次触发** | `onMounted` 立即（`delay = 0`，先立刻请求） |
| **间隔** | 页面可见 **3000 ms**；`document.hidden`（Tab 后台）**15000 ms** |
| **调度方式** | **递归 `setTimeout`**（每次等本次 `load()` 完成后才排下一次），**不是 `setInterval`** → 慢请求不会堆积。React 里必须用同样的递归 setTimeout 模式 |
| **停止条件** | **仅 `onBeforeUnmount`**（`mounted = false` + `clearTimeout(timer)`）。**没有「任务全部就绪就停」的逻辑** —— 只要组件在，永远轮询 |
| **单飞 / 去重** | `loadRequest` promise 哨兵（`:58`）：已有请求在途时 `await loadRequest`；若 `reset` 则 `return load(true)` 重来一次，否则直接返回 |
| **loading 状态** | 每次轮询都 `loading = true → false`，**所以「刷新」按钮每 3 秒闪烁一次 loading**（`:160` `:loading="loading"`）。这是现网行为，重写时可保留或明确改为只在手动刷新时 loading |
| **错误处理** | `catch { error.value = getAdminApiErrorMessage(cause) }`（`:66`），渲染为页面顶部 `AAlert type="error"`（`:162`）。**轮询失败不清空 `data`**，保留上次结果 |
| **`load(true)` 语义** | `reset = true` 时：加载后强制 `apply(current.value)` 重置表单草稿为服务端值。仅在 `save()` / `saveBulk()` 成功后使用 |

**`load` 的两个易漏细节**：

1. `checkedAlbums.value = checkedAlbums.value.filter(id => data.value.settings.some(item => item.albumId === id))`（`:64`）—— **每轮都会与最新 settings 求交集**，防止已删相册残留在批量选择里。
2. `if (reset || !saved.value) apply(current.value)`（`:65`）—— **轮询不会覆盖用户正在编辑的草稿**；只在未初始化（`saved === ''`）或显式 reset 时同步。

## B.10 `embedded` 双上下文（props / 使用位置 / 差异）

### B.10.1 Props / Emits 契约（`:4-5`）

```ts
const props = defineProps<{ albumId?: string, embedded?: boolean }>()
const emit = defineEmits<{ dirty: [value: boolean], busy: [value: boolean] }>()
```

### B.10.2 唯一复用点

`app/pages/dashboard/albums.vue:302`（「公开下载」Tab 内）：

```vue
<ATabPane key="downloads" tab="公开下载">
  <DashboardDownloadManager :key="selectedId" :album-id="selectedId" embedded @dirty="downloadDirty = $event" @busy="downloadBusy = $event" />
</ATabPane>
```

宿主接线：

| 行号 | 内容 |
|---|---|
| `albums.vue:17` | `const tab = computed(() => ['details', 'downloads'].includes(String(route.query.tab)) ? String(route.query.tab) : 'photos')` |
| `albums.vue:26-27` | `const downloadBusy = ref(false)` / `const downloadDirty = ref(false)` |
| `albums.vue:28` | `const locked = computed(() => !!mutation.value || coverBusy.value || downloadBusy.value)` —— **下载保存中会锁住整个相册页**（上传按钮、保存按钮、表单等全部 `:disabled="locked"`） |
| `albums.vue:112` | 路由离开守卫包含 `downloadDirty` |
| `albums.vue:117` | `beforeunload` 守卫包含 `downloadDirty` |
| `albums.vue:124` | 切换相册时重置 `downloadDirty` / `downloadBusy` |
| `albums.vue:208` | 同上重置 |
| `albums.vue:299` | 删除相册按钮 `:disabled="locked \|\| dirty \|\| downloadDirty"` |

### B.10.3 差异表

| 维度 | 独立页（`/dashboard/downloads`，`embedded` 未传 = false） | 内嵌（`albums.vue`，`embedded` = true） |
|---|---|---|
| `selected` 来源 | `route.query.album`（`:11`） | `props.albumId \|\| ''`（`:11`） |
| `pick()` 行为 | `router.push` 改 query（`:72`） | **不会被调用**（相关 UI 全部 `v-if="!embedded"`） |
| `DashboardPageHeader` | 渲染（`:160`） | 不渲染 |
| 相册列表表格卡片 | 渲染（`:163` `v-if="!embedded && !selected"`） | 不渲染 |
| 「切换相册」工具栏 + 「管理此相册图片 →」 | 渲染（`:175` `v-if="!embedded && current"`） | 不渲染 |
| 设置表单 + 本地压缩包卡片 | `v-if="current"`（`:178`，**不判 embedded**） | **同样渲染 ← 内嵌时这才是主体** |
| 「批量设置」入口 | 页头 + 工具栏两处 | **无任何入口**（`openBulk` 只能从独立页触发；内嵌时 `bulkOpen` 恒为 false） |
| `canLeave()`（`:145-149`） | 完整拦截：见下 | **第一行 `if (props.embedded) return true` — 直接放行，不做任何拦截** |
| `beforeunload`（`:155`） | 两种上下文**都注册**：`if (dirty \|\| bulkDirty) { preventDefault; returnValue = '' }` | 同左（内嵌时与 `albums.vue:117` 的监听叠加） |
| 轮询（`:142-144`） | 挂载即轮询 | 同左（**内嵌时 Tab 未激活也照常轮询**，因为 `ATabPane` 默认非 lazy） |
| 事件 | 依旧 emit `dirty` / `busy`，但无人监听 | `albums.vue` 监听并驱动 `locked` 与离开拦截 |

**`canLeave()` 完整实现**（`:145-150`）：

```ts
const canLeave = () => {
  if (props.embedded) return true
  if (saving.value || bulkSaving.value || actionId.value) { notice.add({ title: '正在提交操作，请稍候', color: 'warning' }); return false }
  return (!dirty.value && !bulkDirty.value) || notice.confirm('下载设置尚未保存，确定放弃修改吗？')
}
onBeforeRouteLeave(canLeave)
onBeforeRouteUpdate(canLeave)
```

### B.10.4 emit 接线（`:153-154`）

```ts
watch(dirty, value => emit('dirty', value), { immediate: true })
watch(() => saving.value || !!actionId.value, value => emit('busy', value))
```

- `dirty` 用 **`{ immediate: true }`**，**挂载时立即 emit 一次**（值通常为 false）—— 避免父组件残留旧状态。
- `busy` **没有** `immediate`，且只看 `saving || actionId` —— **不含 `bulkSaving`**（内嵌时批量入口不存在，所以无影响）。
- **`busy` 不含 `loading`** —— 轮询不锁父页面。

### B.10.5 切换相册的重置（`:152`）

```ts
watch(selected, () => { saved.value = ''; history.value = false; bulkOpen.value = false; apply(current.value) })
```

`saved = ''` 使 `dirty` 立即变 false（靠 `!!saved.value` 守卫），`history` 复位，批量弹窗关闭，再 apply 新相册配置。
内嵌时 `albums.vue` 用 `:key="selectedId"` **强制重挂载组件**，所以这条 watcher 主要服务独立页的 `?album=` 切换。

## B.11 相关依赖组件（重写需提供等价物）

| 依赖 | 定义位置 | 签名 / 行为 |
|---|---|---|
| `useAdminNotice()` | `app/composables/useAdminNotice.ts` | `add({ title, description?, color? })` → `color` 映射到 antd notification type（`error` / `warning` / `success` / 其它=info），`placement: 'topRight'`，**duration：error 8 秒，其余 4 秒**。`confirm(content, danger = false): Promise<boolean>` → antd `modal.confirm`，`title: '确认操作'`，`okText: '确认'`，`cancelText: '取消'`，`okButtonProps: { danger }`，`onOk → resolve(true)`，`onCancel → resolve(false)` |
| `useAdminApi()` | `app/composables/useAdminApi.ts` | 见 §A.6.1；`authState` 是 `useState('chronoframe-admin-auth')` 全局态 |
| `useToast()` | **仅为旧组件遗留**：`app/composables/useAdminNotice.ts` 是新版；`DownloadManager.vue` 用的是 `useAdminNotice()`（`:7`） | — |
| `DashboardPageHeader` | `app/components/dashboard/PageHeader.vue` | 自动导入（`nuxt.config.ts:20`） |
| `adminBytes` / `downloadStatus` / `downloadStatusColor` | `app/utils/adminFormat.ts` | 自动导入，无 import 语句 |
| `useEventListener` | VueUse（自动导入） | `beforeunload` 注册 |

## B.12 下载管理完整 UI 结构树

```
downloads.vue（6 行壳：definePageMeta layout=dashboard, useHead title='下载管理'）
└── <DashboardDownloadManager />   ← DownloadManager.vue，props 全缺省（embedded=false）
    ├── [v-if="!embedded"] DashboardPageHeader  标题=「{albumName} · 下载设置」或「下载管理」
    │                        #default: [selected 时]返回下载列表 / 刷新(:loading=loading)
    └── div.admin-stack
        ├── AAlert type=error   v-if="error"
        ├── ACard  v-if="!embedded && !selected"        ← 相册列表视图
        │   ├── .admin-toolbar
        │   │   ├── ASpace wrap: AInputSearch(搜索相册) / 批量设置(N) / [有勾选]取消选择
        │   │   └── AStatistic 本地 ZIP 占用 = adminBytes(localBytes)
        │   └── ATable row-key=albumId，row-selection(preserveSelectedRowKeys)，pageSize 20
        │       └── 列：相册(link) / 公开下载(Tag) / 格式(Tag×n) / 当前任务(albumStatus) / 操作(管理下载)
        ├── div.admin-toolbar  v-if="!embedded && current"   ← 切换相册 + 管理此相册图片 →
        ├── AAlert warning  v-if="selected && !current && !loading && !error"   相册不存在
        ├── AAlert info     v-if="!data.settings.length && !loading"             请先创建相册
        ├── div.admin-settings-grid  v-if="current"
        │   ├── ACard「下载设置」
        │   │   └── AForm @finish=save，:disabled=saving
        │   │       ├── 可供下载        ASwitch
        │   │       ├── 图片格式        ACheckboxGroup(PNG/JPG/JPEG/WEBP) required
        │   │       ├── 单张大小上限(MB) AInputNumber 0..500 step 0.5
        │   │       ├── [保存下载设置 :disabled=!dirty] [放弃修改 :disabled=!dirty||saving]
        │   │       └── p.admin-help  v-if="dirty"  有未保存的修改
        │   └── ACard「本地压缩包」  #extra=[重新生成 :disabled=!current.enabled]
        │       ├── p.admin-help 服务器后台运行 / ZIP 仅存本机
        │       ├── .admin-toolbar 当前版本|全部版本 + ACheckbox 显示历史记录
        │       ├── ATable row-key=id pageSize 8
        │       │   └── 列：格式(+v{revision}) / 状态·进度(Tag+Progress+error) / 文件大小 / 生成时间 / 操作(下载·取消·删除)
        │       └── p.admin-help 存放位置 data.directory
        └── AModal「批量设置相册下载」 :open=bulkOpen width=640 footer=null
            └── AForm @finish=saveBulk
                ├── AAlert warning 统一覆盖各相册的单独设置
                ├── 应用范围        ARadioGroup(选定相册 | 全部相册(N))
                ├── [selected 时]选择相册  ASelect mode=multiple :max-tag-count=4
                ├── p.admin-help 将覆盖 {bulkCount} 个相册
                ├── 可供下载 / 图片格式 / 单张大小上限
                └── [取消] [覆盖并应用（N）html-type=submit :disabled=!bulkCount||!formats.length]
```

---

# C. 格式转换：现状说明（已从重写范围剔除）

## C.1 前端现状

`app/pages/dashboard/conversions.vue` 当前是 **6 行纯重定向，无任何 UI**：

```vue
<script setup lang="ts">
definePageMeta({ layout: 'dashboard' })
await navigateTo('/dashboard/settings/storage', { replace: true })
</script>

<template><div /></template>
```

- 无 `useHead`。在 commit **`fbe6436`**（`feat: add three-tier image delivery and photo exports (#9)`）中由 **766 行删减为 6 行**（`git show fbe6436 -- app/pages/dashboard/conversions.vue` 显示整页删除）。
- 当前 `app/` 下**没有任何组件引用格式转换**。
- `/dashboard` 侧边栏（`app/layouts/dashboard.vue:17-24`）也**没有**格式转换入口。菜单项为：概览 `/dashboard`、相册管理 `/dashboard/albums`、下载管理 `/dashboard/downloads`、任务中心 `/dashboard/tasks`、存储与维护 `/dashboard/settings/storage`、网站设置 `/dashboard/settings/general`。
- `app/pages/dashboard/settings/storage.vue` 的四个 Tab 是「存储连接 / 存储迁移 / 图片缓存（三层派生图重建）/ S3 空间清理」，**不含格式转换**。

## C.2 残留物

`app/types/dashboard.ts:40-82` 仍保留以下类型定义，**全仓库无任何引用**（仅 `app/types/dashboard.ts` 与 `backend/src/main.rs` 命中 `ConversionJob`）：

- `ConversionJob`（`:40-56`）—— 含 `sourcesDeletedAt: number | null`、`sourceDeleteTotal` / `sourceDeleteCompleted` / `sourceDeleteRemaining` / `sourceDeleteFailed`
- `ConversionItem`（`:58-65`）
- `ConversionDetail`（`:67-70`）
- `SourceDeletionFailure`（`:72-75`）
- `SourceDeletionResult`（`:77-82`）

**重写时可原样保留这些类型定义**（零成本，便于将来复活），但**不要为它建页面、不要设计 UI**。

> 历史参考：旧实现（766 行）可用 `git show fbe6436^ -- app/pages/dashboard/conversions.vue` 取回。`sourcesDeletedAt` 的旧状态机语义为：`null` = 未删除（仍需人工确认）、负数 = 后台处理中（其中 `-2` 为「服务重启后待恢复」态）、正数 = 已由管理员确认删除的时间戳。

## C.3 后端契约路径清单（供将来复活参考）

`backend/src/main.rs:6924-6935`：

| method | 路径 | handler |
|---|---|---|
| GET | `/api/conversions` | `list_conversions` |
| POST | `/api/conversions` | `start_conversion` |
| GET | `/api/conversions/{job_id}` | `get_conversion` |
| POST | `/api/conversions/{job_id}/cancel` | `cancel_conversion` |
| DELETE | `/api/conversions/{job_id}/delete-sources` | `confirm_delete_sources` |

补充说明（仅路径级信息，**不提供请求/响应细节**）：

- 后端结构体 `ConversionJob` 的字段与 `app/types/dashboard.ts:40-56` 一一对应（`#[serde(rename_all = "camelCase")]`）。
- `confirm_delete_sources` 只在 `status ∈ {completed, failed, cancelled, interrupted}` 且 `succeeded > 0` 时接受请求，会占用 `storage_mutation_gate` 与 `photo_graph_lock`；存储迁移/清理进行中返回 409「存储正在迁移或清理，请稍后再删除旧格式图片」。
- `GET /api/conversions/{job_id}` 支持 `items` query 参数控制是否返回条目明细。

---

# 附录：迁移给 React 的注意点汇总（易踩坑清单）

1. **`shared/utils/admin-upload-queue.ts` 是纯函数，可 1:1 复用。** 切勿在重写时「顺手」加进度 / 取消 / 重试计数 —— 那会改变与后端契约及全部文案的一致性。
2. **`pump()` 的原子占位**：`slots--` 与 `status = 'uploading'` 必须同步完成。用 `useEffect` 派发异步任务极易破坏这一点，导致并发超过 7。
3. **`resume()` 必须 pump**（后台 `pump()` 只在 enqueue / resume / retryFailed / 任务完成时被调用）；**`pause()` 必须不 abort 在途请求**。
4. **`beforeunload` 的 `event.returnValue = ''`** 在现代浏览器已被忽略，但必须保留 `preventDefault()`；保持原样。
5. **`adminBytes`（1000 进制，`app/utils/adminFormat.ts`）与 `formatBytes`（1024 进制，`app/pages/dashboard/settings/storage.vue:298-304`）是两个不同函数**，不要合并。
6. **`downloadStatus` / `downloadStatusColor` 在 `app/utils/adminFormat.ts`**，不在组件内，迁移时容易漏。
7. **`useAdminNotice().confirm()` 返回 `Promise<boolean>`，第二参数 = danger 样式**；`add()` 的 `color` 映射到 notification type，**error 8 秒、其余 4 秒**，位置 `topRight`。全仓库大量依赖，重写必须提供同签名 API。
8. **下载页 URL 是唯一真相**（`?album=`），别用 React state 镜像路由。
9. **下载轮询是递归 `setTimeout`（可见 3s / 隐藏 15s）且永不自动停止**（无「全部就绪即停」逻辑）；`load()` 每次都会闪 `loading`。
10. **`DownloadManager` 每 3 秒 `load()`，但 `apply()` 只在 `reset || !saved` 时执行** —— 这是「轮询不打断用户编辑」的关键，极易在重写时丢失。
11. **`DownloadManager` 内嵌时 `canLeave()` 直接 `return true`**，拦截责任转移给宿主 `albums.vue`；同时 `busy` emit **不含 `bulkSaving` 也不含 `loading`**。
12. **批量设置 `{scope:'all'}` 绝不能带 `albumIds` 键**（后端 `deny_unknown_fields` + internally-tagged enum 会直接报错）。
13. **`job.revision === settings.revision` 才算「当前版本」**，丢了这条历史 ZIP 会污染当前状态。
14. **上传文件名正则含 `jepg` 拼写错误**（`albums.vue:47` 与 `:168`），属现网行为，改动即行为变更。
15. **`save()` / `saveBulk()` 成功后用 `load(true)`**（重置草稿），**`jobAction()` / `rebuild()` 用 `load()`**（不重置草稿）—— 这个区别不要弄混。
