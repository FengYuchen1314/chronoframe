# 管理后台 React 重写规格（Shell 层：布局 / 导航 / 概览 / 任务中心 / 通知）

> 唯一依据文档。所有引用均来自实际源码（含行号），逐字摘录，未做猜测性补充。
> 分析对象（只读，未修改）：
> - `app/layouts/dashboard.vue`（87 行）
> - `app/pages/dashboard/index.vue`（59 行）
> - `app/pages/dashboard/tasks.vue`（79 行）
> - `app/composables/useAdminNotice.ts`（14 行）
> - 关联依赖：`app/composables/useAdminApi.ts`、`app/composables/useAdminUploads.ts`、`app/composables/useSiteSettings.ts`、`app/components/dashboard/PageHeader.vue`、`app/components/dashboard/UploadQueue.vue`、`app/assets/css/admin.css`、`app/types/dashboard.ts`、`shared/types/downloads.ts`、`shared/utils/admin-upload-queue.ts`、`app/utils/adminFormat.ts`、`app/pages/dashboard/albums.vue`、`app/pages/dashboard/settings/storage.vue`、`backend/src/main.rs`、`backend/src/album_downloads.rs`

---

## 0. 目标工程与技术栈映射（重写约束）

### 0.1 目标工程事实（`admin/`）

来自 `admin/package.json`、`admin/vite.config.ts`、`admin/tsconfig.json`、`admin/index.html`：

| 项 | 事实 |
|---|---|
| 形态 | **独立构建的 React SPA**（不是嵌进 Nuxt），`admin/vite.config.ts:5-8` 注释：产出 `admin/dist`，再由构建脚本落到 `public/dashboard`，随 Nuxt 静态产物发布到 `/dashboard/` |
| base | `base: '/dashboard/'`（`vite.config.ts:10`）→ react-router 需 `basename="/dashboard"`；静态托管需保证 `/dashboard/*` 的 SPA fallback |
| dev server | 端口 5174，`/api` 代理到 `http://127.0.0.1:8080`（`vite.config.ts:17-26`，注释：「以便同源携带 Cookie」） |
| 框架 | React 19 + react-router-dom 7 + Tailwind CSS v4（`@tailwindcss/vite`） |
| UI 库 | **HeroUI `@heroui/react ^3.2.5`**（不是 antd） |
| 图标 | `@iconify/react` + `@iconify-json/tabler`（源项目图标名即 `tabler:*`，**名字可直接复用**） |
| 路径别名 | `~/* → ./src/*`（`admin/tsconfig.json`） |
| 入口 | `admin/index.html` 引 `/src/main.tsx`；`lang="zh-CN"`、`title="管理后台 · ChronoFrame"`、`robots noindex,nofollow`、`color-scheme: light dark` |
| 构建 | `dev` / `build` / `preview` / `typecheck`；无 lint |

### 0.2 Vue → React 语义映射表

> 目标库是 HeroUI v3；下表给**语义映射**（组件名以 HeroUI v3 实际 API 为准），重点是**尺寸、断点、文案、状态**必须等价。

| Vue 侧 | 语义 | React/HeroUI 侧 |
|---|---|---|
| `AConfigProvider(locale=zhCN, theme.token={colorPrimary:'#1677ff', borderRadius:6, fontFamily:'-apple-system,BlinkMacSystemFont,Segoe UI,Noto Sans SC,sans-serif'})`（`dashboard.vue:52`） | 主题令牌 + 中文语言包 | `<HeroUIProvider>` + Tailwind v4 主题令牌；文案全硬编码中文，**不需要语言包**。主色 `#1677ff`、圆角 6px、同一字体栈必须显式落成 CSS 变量 |
| `AApp` + `App.useApp()`（`useAdminNotice.ts:4`） | 通知/弹窗宿主单例 | HeroUI `ToastProvider`/`toast` + 受控 `Modal`；**必须挂在 shell 根部**，通知与确认弹窗是应用级单例 |
| `AButton` | 按钮 | HeroUI `Button`；属性对应：`:loading`→`isLoading`、`:disabled`→`isDisabled`、`type="primary"`→`color="primary"`、`type="text"/"link"`→`variant="light"/"ghost"`、`block`→`fullWidth`、`danger`→`color="danger"`、`html-type="submit"`→`type="submit"` |
| `ASpace` | flex 间隔容器 | `flex gap-*`；默认 gap 8，顶栏右侧 `:size="12"` |
| `ACard`（`title=` 属性 / `size="small"` / `:bordered=false`） | 卡片 | HeroUI `Card` + `CardHeader/CardBody` |
| `ATable`（`columns`/`data-source`/`row-key`/`:loading`/`:pagination`/`:scroll={x}`/`#bodyCell`） | 表格 | HeroUI `Table`（TableHeader/TableColumn/TableBody/TableRow/TableCell）；`:scroll.x` 需保留横向滚动容器；分页为独立组件 |
| `AStatistic`（`title`/`value`/`:value-style`） | 统计数字 | 无直接等价物 → 自定义（标题小字 + 大数字；`当前图片存储` 用 `fontSize:22`） |
| `AAlert`（`type`/`show-icon`/`:message`/`:description`/`closable`） | 内联提示 | HeroUI `Alert` 或自定义；**必须保留 `.ant-alert{white-space:pre-wrap}`（`admin.css:19`）的多行换行能力** |
| `ATag`（`:color`） | 状态标签 | HeroUI `Chip`；颜色映射 `orange`→warning、`processing`→primary(带脉冲动画，源站为 antd "processing" 样式)、`default`→default |
| `AProgress`（`:percent`/`:status='exception'`/`size="small"`） | 进度条 | HeroUI `Progress`（`value`/`maxValue`/`color`）；`exception`→`color="danger"` |
| `AEmpty`（`description`） | 空态 | 自定义空态组件 |
| `ARadioGroup(option-type="button")` | 分段筛选 | HeroUI `Tabs` 或自定义分段按钮组；`aria-label="任务状态筛选"` 必须保留 |
| `ADrawer`（`placement`/`:width`/`title`/`:body-style={padding:0}`） | 抽屉 | HeroUI `Drawer`；移动导航宽 **248**、左侧；上传队列宽 **`min(720px,100vw)`**、右侧 |
| `ASpin`（`tip`） | 加载态 | HeroUI `Spinner` |
| `ABadge(:dot)` | 小红点 | HeroUI `Badge` |
| `AMenu(mode="inline" :items :selected-keys @click)` | 侧栏菜单 | 自定义 nav 列表（6 项，无分组）；激活判定见 §1.3 |
| `ABreadcrumb` | 面包屑 | HeroUI `Breadcrumbs` |
| `AVatar`（`shape="square"`） | 头像 | HeroUI `Avatar`（方角/圆角 6） |
| `App.useApp().notification` | 顶右通知 | HeroUI `toast`；**error 8s，其他 4s，可堆叠** |
| `App.useApp().modal.confirm` | 确认弹窗 | 受控 `Modal` + Promise；固定标题「确认操作」、按钮「确认」/「取消」、`danger` 映射 `color="danger"` |
| `Icon name="tabler:xxx"` | 图标 | `<Icon icon="tabler:xxx" />`，名字不变 |
| `useMediaQuery('(max-width: 991px)')` | 断点 | `matchMedia('(max-width: 991px)')` hook。**不要用 Tailwind 默认 `lg=1024px` 代替**，否则布局切换点从 991 变成 1024 |
| `useState(key, init)` | 应用级单例状态 | Context/store，key 清单见 §6；**跨路由切换不得重置** |
| `onBeforeRouteLeave` / `onBeforeRouteUpdate` | 未保存离开保护 | react-router v7 `useBlocker` |
| `useHead({title})` + `app.vue:82` `titleTemplate` | 文档标题 | `document.title = \`${pageTitle} \| ${siteSettings.title}\`` |
| Nuxt `$fetch` | HTTP | `fetch`，`credentials:'include'`，头 `X-Requested-With: ChronoFrame` |

**全局行为约束（源项目事实，重写须等价）**
- 源项目 `ssr: false`（`nuxt.config.ts:6`）→ 纯 CSR；目标本就是 Vite SPA。
- 管理后台**全部硬编码中文**：`app/pages/dashboard/**`、`app/layouts/`、`app/components/dashboard/` 内 grep `$t(`/`useI18n` **零命中**。不要接 i18n。
- 管理后台**恒为浅色**（`theme.defaultAlgorithm` + `admin.css` 全固定浅色值），不跟随暗色主题。
- 管理后台**无路由中间件**：鉴权全部由 shell 承担（未登录时页面组件根本不挂载）。
- 无 keep-alive：切页即重新挂载并重新请求（唯一例外是 §6 的 8 个共享 key）。
- **所有 API 路径保持相对 `/api/...`**：dev 走 Vite 代理，生产同源（`cf_session`/`cf_csrf` 为 `SameSite=Strict`，跨源会直接丢 Cookie）。

---

## 1. `app/layouts/dashboard.vue`（shell 主体，87 行）

### 1.1 UI 结构与层级（逐行）

```
AConfigProvider(locale=zhCN, theme={algorithm: defaultAlgorithm, token:{colorPrimary:'#1677ff', borderRadius:6,
                fontFamily:'-apple-system,BlinkMacSystemFont,Segoe UI,Noto Sans SC,sans-serif'}})          // L52
└ AApp.admin-app                                                                                          // L53
  ├ [分支A] div.admin-auth > ASpin(size="large", tip="正在检查登录状态")                                    // L54
  │         条件：!authState.checked || authState.loading
  ├ [分支B] div.admin-auth > ACard.admin-auth-card(:bordered="false")                                       // L55-70  未登录
  │   ├ 头部行(flex items-center gap-3)：AAvatar(shape="square" :src=settings.avatarUrl)
  │   │     + <strong>{{settings.title}}</strong> + span.admin-help「管理后台」                            // L57
  │   ├ <h1>{{ registration ? '创建管理员账号' : '管理员登录' }}</h1>                                        // L58
  │   ├ <p class="admin-help">{{ registration ? '首次使用，请创建唯一的管理员账号。' : '管理相册、公开下载与网站设置。' }}</p>  // L59
  │   ├ AAlert(type="error" show-icon :message="authState.error || error")  v-if="authState.error || error" // L60
  │   ├ AForm(layout="vertical" :model="form" @finish="submit")                                            // L61-66
  │   │   ├ AFormItem「用户名」 required → AInput(size="large" autocomplete="username" :maxlength=64)        // L62
  │   │   ├ AFormItem「密码」   required → AInputPassword(size="large" :autocomplete="registration?'new-password':'current-password'" placeholder="至少 12 个字符")  // L63
  │   │   ├ AFormItem「确认密码」 required v-if="registration" → AInputPassword(autocomplete="new-password") // L64
  │   │   └ AButton(html-type="submit" type="primary" size="large" block :loading="busy")
  │   │       文案：registration ? '创建账号并进入后台' : '登录'                                            // L65
  │   ├ AButton(block class="mt-3" @click="refreshAuthStatus")「重试连接」  v-if="authState.error"           // L67
  │   └ div.mt-5.text-center > NuxtLink(to="/")「返回相册」                                                  // L68
  └ [分支C] ALayout                                                                                        // L71-84  已登录
      ├ ALayoutSider.admin-sider v-if="!mobile" :width="224" theme="light"                                  // L72-75
      │   ├ NuxtLink.admin-brand(to="/dashboard") → <img :src=settings.avatarUrl alt=""> + <span>{{settings.title}}</span>  // L73
      │   └ AMenu(mode="inline" :items="items" :selected-keys="[route.path]" @click="navigate")             // L74
      ├ ADrawer(v-model:open="menuOpen" placement="left" :width="248" title="管理后台" :body-style={padding:0})  // L76
      │   └ AMenu(mode="inline" :items="items" :selected-keys="[route.path]" @click="navigate")             // L76（桌面端也挂载，但无按钮可打开）
      └ ALayout(style="min-width:0")                                                                        // L77
          ├ ALayoutHeader.admin-topbar                                                                      // L78-81
          │   ├ 左 ASpace：AButton(type="text" aria-label="打开导航" v-if="mobile" @click="menuOpen=true")
          │   │           + ABreadcrumb[ ABreadcrumbItem「管理后台」, ABreadcrumbItem{{title}} ]             // L79
          │   └ 右 ASpace(:size=12)：<DashboardUploadQueue/>
          │           + NuxtLink(to="/" target="_blank")「查看网站」 v-if="!mobile"
          │           + span.admin-help {{authState.username}} v-if="!mobile"
          │           + AButton(:loading="busy" @click="signOut")「退出」                                   // L80
          └ ALayoutContent.admin-content                                                                    // L82
              ├ AAlert(v-if="error" type="error" :message="error" closable @close="error=''")
              └ <slot />   ← 页面内容
```

`admin.css` 中与 shell 直接相关的尺寸（重写必须复刻）：

| 选择器 | 规则 | 行 |
|---|---|---|
| `.admin-app` | `min-height:100svh; color:rgba(0,0,0,.88); background:#f5f5f5; font-family:-apple-system,BlinkMacSystemFont,'Segoe UI','Noto Sans SC',sans-serif` | L1 |
| `.admin-app *` | `box-sizing:border-box` | L2 |
| `.admin-app .ant-layout` | `min-height:100svh; background:#f5f5f5` | L3 |
| `.admin-sider` | `position:sticky; top:0; height:100svh; border-right:1px solid #f0f0f0` | L4 |
| `.admin-brand` | `height:64px; padding:0 24px; display:flex; align-items:center; gap:10px; font-size:17px; font-weight:600; color:#141414; overflow:hidden; white-space:nowrap`；`img{width:28px;height:28px;border-radius:6px}` | L5-6 |
| `.admin-topbar` | `padding:0 24px; height:64px; line-height:normal; display:flex; align-items:center; justify-content:space-between; gap:16px; background:white; border-bottom:1px solid #f0f0f0` | L7 |
| `.admin-content` | `padding:24px; min-width:0; width:100%; max-width:1680px; margin:0 auto` | L8 |
| `.admin-page-header` | `display:flex; align-items:flex-start; justify-content:space-between; gap:20px; margin-bottom:24px`（h1：24px/600/1.4/`rgba(0,0,0,.88)`；p：`margin:8px 0 0`、`rgba(0,0,0,.45)`、14px） | L9-11 |
| `.admin-page-actions` | `display:flex; flex-wrap:wrap; align-items:center; justify-content:flex-end; gap:8px` | L12 |
| `.admin-stack` | `display:flex; flex-direction:column; gap:24px` | L13 |
| `.admin-toolbar` | `display:flex; flex-wrap:wrap; justify-content:space-between; align-items:center; gap:12px; margin-bottom:16px` | L14 |
| `.admin-help` | `color:rgba(0,0,0,.45); font-size:13px; line-height:1.7` | L17 |
| `.admin-app .ant-alert` | **`white-space:pre-wrap`**（多行错误依赖此项） | L19 |
| `.admin-auth` | `display:grid; place-items:center; min-height:100svh; padding:24px; background:#f0f2f5` | L22 |
| `.admin-auth-card` | `width:100%; max-width:420px`；h1 `font-size:24px;font-weight:600;margin:16px 0 8px`；`.ant-form{margin-top:24px}` | L23-25 |
| `.admin-field-error` | `color:#cf1322; overflow-wrap:anywhere; font-size:13px; margin-top:4px` | L52 |
| `.admin-upload-summary` | `display:flex; flex-wrap:wrap; gap:16px; margin:20px 0 8px` | L51 |
| `@media(max-width:768px)` | `.admin-content{padding:16px}`、`.admin-topbar{padding:0 16px}`、`.admin-page-header{flex-direction:column;gap:16px}`、`.admin-page-actions{justify-content:flex-start}`、`.admin-form-grid{grid-template-columns:1fr}` | L62 |

`DashboardPageHeader`（`app/components/dashboard/PageHeader.vue:5-8`）：
```html
<header class="admin-page-header">
  <div><h1>{{title}}</h1><p v-if="description">{{description}}</p></div>
  <div class="admin-page-actions"><slot/></div>
</header>
```

### 1.2 响应式行为（精确，移动端问题在此明确回答）

`const mobile = useMediaQuery('(max-width: 991px)')`（L10）→ **≤991px 即移动端**。

| 元素 | 桌面 >991px | 移动 ≤991px |
|---|---|---|
| 左侧侧栏 `ALayoutSider`（224px，sticky，满高 100svh） | 显示 | **`v-if="!mobile"` → DOM 中完全不存在**。既不是 mini 折叠、也不是 0 宽，而是**移除** |
| 左侧抽屉 `ADrawer`（`placement="left"`，`width=248`，`title="管理后台"`，body padding 0） | 组件挂载但 `open=false`，且**没有任何按钮能打开它** | **唯一的导航入口**：汉堡按钮 → `menuOpen=true` |
| 顶栏汉堡按钮（`aria-label="打开导航"`） | 不渲染（L79 `v-if="mobile"`） | 渲染 |
| 顶栏「查看网站」外链 | 显示 | **隐藏**（L80 `v-if="!mobile"`） |
| 顶栏用户名 `span.admin-help` | 显示 | **隐藏**（L80）。用户名**不迁移到抽屉里**，移动端无处可见 |
| 顶栏上传队列按钮 `<DashboardUploadQueue/>` | 显示 | **仍然显示**（无 v-if） |
| 顶栏「退出」按钮 | 显示 | **仍然显示**（无 v-if） |
| 面包屑「管理后台 / {title}」 | 显示 | 显示（与汉堡同一 `ASpace`） |

补充断点（`admin.css`）：`≤768px` 内容区 padding 24→16、顶栏 padding 24→16、页面头改为纵向堆叠且按钮左对齐；`≤1100px` 快捷卡片 3 列→1 列（L59）；`≤600px` 另有图片网格/保存条等规则（与本 shell 文件无关）。

**结论（明确回答）**：后台布局在移动端是**抽屉（Drawer）**，且侧栏被**直接卸载**而不是折叠。桌面端抽屉存在但不可达。

### 1.3 导航信息架构（完整）

菜单项定义（L17-24）——**单一扁平列表，无分组、无嵌套、无子菜单**，`key` 即路由 path：

| # | `key`（路由） | `label` | `icon`（Iconify 名字） |
|---|---|---|---|
| 1 | `/dashboard` | 概览 | `tabler:dashboard` |
| 2 | `/dashboard/albums` | 相册管理 | `tabler:album` |
| 3 | `/dashboard/downloads` | 下载管理 | `tabler:download` |
| 4 | `/dashboard/tasks` | 任务中心 | `tabler:activity` |
| 5 | `/dashboard/settings/storage` | 存储与维护 | `tabler:database` |
| 6 | `/dashboard/settings/general` | 网站设置 | `tabler:settings` |

- **激活态判定**（L74、L76）：`:selected-keys="[route.path]"` —— **只用 pathname 精确匹配**，忽略 query 与 hash。因此 `/dashboard/albums?album=x&tab=downloads` 仍然高亮「相册管理」；`/dashboard/settings/storage?tab=cache` 高亮「存储与维护」。
- **点击行为**（L26）：`const navigate = ({key}) => { menuOpen.value = false; void router.push(String(key)) }` —— 关抽屉 + `push`（**新增历史记录**，非 replace）。
- **面包屑**（L25）：`title = computed(() => items.find(i => i.key === route.path)?.label || '管理后台')`。未登记路径（如 `/dashboard/conversions`，该页仅 `navigateTo('/dashboard/settings/storage', {replace:true})`）→ 面包屑显示「管理后台 / 管理后台」，且菜单无高亮。
- **登录态显示逻辑**：三态渲染（L54 spinner / L55 登录卡 / L71 后台）。`registration = computed(() => !authState.value.initialized)`（L16）决定表单是「创建管理员账号」还是「管理员登录」，从而决定：确认密码字段显隐（L64）、密码框 `autocomplete`（L63）、提交按钮文案（L65）、`<h1>` 与说明文案（L58-59）。`initialized` 来自 `GET /api/auth/status`。
- 侧栏品牌区是 `NuxtLink to="/dashboard"`（L73，回概览）。
- 无「设置」父级、无折叠按钮、无主题/语言切换入口。

### 1.4 API 调用清单（本文件直接/间接触发）

统一 HTTP 层（`app/composables/useAdminApi.ts`）：
- 头：恒带 `X-Requested-With: ChronoFrame`（`:108`）；`credentials:'include'`（`:120`）。
- 非 GET/HEAD：追加 `X-CSRF-Token`，值读自可读 cookie **`cf_csrf`**（`:111-112`，`readBrowserCookie` 做 `decodeURIComponent`）。
- 错误对象形状（`:26-37`）：`{ data?:{error?,message?}, message?, status?, statusCode?, response?.status? }`。
- `getAdminApiErrorMessage()`（`:65-76`）取值顺序：`data.error → data.message → message → '请求失败，请稍后重试'`（字符串直接返回）。
- 后端错误体恒为 `{"error":"<message>"}`（`backend/src/main.rs:2028-2052` `IntoResponse for AppError`）。

| # | 触发点 | Method | 路径（含 query） | Body | 期望响应 |
|---|---|---|---|---|---|
| 1 | `onMounted`（L47）、「重试连接」（L67）、register/login/logout 内部 | GET | `/api/auth/status` | — | `{ initialized: boolean, authenticated: boolean, username?: string }`。**`username` 为空时字段被省略**（后端 `#[serde(skip_serializing_if="Option::is_none")]`，main.rs:2066-2073）。**此请求走裸 `request`，不走 `adminFetch`**，无需已登录 |
| 2 | `submit()` 注册分支（L35） | POST | `/api/auth/register` | `{ username: string(已 trim), password: string }` | 200 `{initialized:true, authenticated:true, username}` + `Set-Cookie: cf_session`(HttpOnly)、`cf_csrf`(可读)，均 `Path=/; SameSite=Strict; Max-Age=<TTL>`。错误：400 校验失败、**409 `管理员账号已经注册`** |
| 3 | `submit()` 登录分支（L36） | POST | `/api/auth/login` | 同上 | 同 #2 成功形状；**401 `用户名或密码错误`**、409 `尚未注册管理员账号`（main.rs:2524-2553） |
| 4 | `signOut()`（L44） | POST | `/api/auth/logout` | 无请求体 | `{ ok: true }` + 两个过期 Cookie（main.rs:2555-2580） |
| 5 | `onMounted` → `ensureSiteSettings()`（L47） | GET | `/api/settings/site` | — | `SiteSettings = { title, slogan, author, avatarUrl, theme:'light'\|'dark'\|'system' }`（`app/types/dashboard.ts:187-193`）。**公开端点**；`useSiteSettings.ts:38-40` 用裸 `$fetch`，**不经 `adminFetch`** |

`adminFetch` 前置（`useAdminApi.ts:199-212`）：若 `!authState.checked || authState.loading` → 先 `await refreshAuthStatus()`；若仍 `!authenticated` → **不发任何请求**，直接 `throw new Error('请先登录管理员账号')`。
**401 全局处理**（`useAdminApi.ts:122-125`）：任何请求收到 401 → `markUnauthenticated()`（`:96-100`：`checked=true, authenticated=false, error=''`，**不写错误文案**）→ shell 立刻切回登录卡，页面组件卸载。

并发去重：`refreshAuthStatus` 用模块级 `pendingAuthStatusRequest` 保证同一时刻只有一个 `/api/auth/status`（`:39`、`:128-152`）。

后端登录态校验（供参考）：`require_requested_with` 校验 `X-Requested-With`；写操作额外校验 CSRF（main.rs:2209 等）。

### 1.5 全部用户交互

1. **表单提交**（`submit`，L27-40，绑定 `@finish`，即原生 submit）：
   - `if (busy.value) return`（L28，防重复提交）。
   - 清空 `error`（L29）。
   - 客户端校验（**中文文案逐字保留**）：
     - `!form.username.trim() || form.username.trim().length > 64` → `用户名需为 1–64 个字符`（L30，注意是**短横线 en dash `–`**）
     - `form.password.length < 12 || new TextEncoder().encode(form.password).byteLength > 1024` → `密码至少 12 个字符，最多 1024 字节`（L31）
     - 注册模式两次密码不一致 → `两次输入的密码不一致`（L32）
   - `busy=true` → 注册或登录 → 成功后**再校验** `authState.authenticated`，若为 false → `error = authState.error || '登录状态未生效，请重试'`（L37）。
   - `finally`：`busy=false`；**清空 password 与 confirmation**（L39，username 保留）。
   - `busy` 同时驱动提交按钮 loading（L65）与顶栏「退出」loading（L80，同一个变量）。
2. **重试连接**（L67）：`v-if="authState.error"`，`@click="refreshAuthStatus"`，**无 loading 态**。
3. **退出登录**（`signOut`，L41-46）：
   - 若 `uploads.pending.value || uploads.failed.value`（`pending` = uploading+queued，`useAdminUploads.ts:22`）→ `uploads.open.value = true`（**强制打开全局上传队列抽屉**）+ `error = '请先处理上传队列，再退出登录。'` + `return`（**不发请求**）。
   - 否则 `busy=true` → `logout()` → `catch` 写 `error` → `finally busy=false`。
   - **登出后不跳转路由**：靠 `markUnauthenticated()` 让 shell 切回登录卡（页面卸载）。因此 `albums.vue` 的未保存保护**不会**触发。
4. **菜单点击**（L26）：关抽屉 + `router.push`。
5. **汉堡按钮**（L79）：`menuOpen = true`。
6. **抽屉**：受 `menuOpen` 控制；点击菜单项即关闭（navigate 内）。
7. **内容区错误 Alert**（L82）：`closable`，`@close="error = ''"`。
8. **品牌链接 / 查看网站（新标签）/ 返回相册** 三个导航元素（L73、L80、L68）。
9. `watch(registration, ...)`（L48）：注册态→登录态切换（如 409 竞态后自动刷新）时清空 password/confirmation。

### 1.6 状态管理（本文件）

| 名称 | 定义 | 含义 |
|---|---|---|
| `authState` | `useState('chronoframe-admin-auth')`（`useAdminApi.ts:79`）**全局单例** | `{checked, loading, initialized, authenticated, username, error}`；三态渲染、面包屑、按钮显示的全部依据 |
| `settings` | `useState('chronoframe-site-settings')`（`useSiteSettings.ts:14`）**全局单例** | 登录卡与侧栏显示 `title`/`avatarUrl` |
| `mobile` | `useMediaQuery('(max-width: 991px)')` 本地 | 响应式开关 |
| `menuOpen` | `ref(false)` 本地 | 抽屉开关（切页重置；由 watcher/导航关闭） |
| `busy` | `ref(false)` 本地 | 提交 / 退出共用 loading |
| `error` | `ref('')` 本地 | **双位置复用**：登录卡内 AAlert（L60）与内容区可关闭 AAlert（L82） |
| `form` | `reactive({username:'', password:'', confirmation:''})` 本地 | password/confirmation 在提交 finally（L39）与注册态切换（L48）时清空；username 跨模式保留 |
| `registration` | `computed(!authState.initialized)` L16 | 注册 / 登录模式 |
| `title` | `computed` L25 | 面包屑第二项 |
| `uploads` | `useAdminUploads()`（全局 useState 支撑，§6） | 仅用于「退出前拦截」与顶栏按钮 |

### 1.7 错误处理与边界情况

- **401（任意 admin 请求）→ 静默登出**（无提示文案，界面直接回到登录卡）。这是全局机制，不要改成弹窗或跳转。
- **register 遇 409** → `useAdminApi.ts:163` 先 `await refreshAuthStatus()` 再 rethrow：并发注册竞态时本标签页**自动从注册切到登录**（并触发 L48 清空密码）；用户看到 `authState.error || error` = `管理员账号已经注册`。
- **login 401** → `error` 显示后端文案 `用户名或密码错误`（经 `data.error`）。
- **logout 特殊流程**（`useAdminApi.ts:177-197`）：首次 `POST /api/auth/logout`；若 **403** → `refreshAuthStatus()` 后**重试一次**；重试若为 401 则吞掉视为成功，其他错误抛出；首次为 **401** → 视为成功；其他状态码 → 抛出。最终无论如何执行 `markUnauthenticated()`。
- `refreshAuthStatus` 失败：`checked=true, authenticated=false, error=后端文案` → 登录卡 + 错误 Alert + 「重试连接」按钮。
- 边界：`settings.avatarUrl` 未加载时用默认 `/web-app-manifest-192x192.png`（`useSiteSettings.ts:3-9`）。
- 边界：`authState.username` 为空时桌面端渲染空 span（无占位符）。
- 边界：`onMounted` 的 `void ensureSiteSettings()` **未 catch**（L47）→ 失败会成为未处理的 Promise rejection（`useSiteSettings.refreshSiteSettings` 会 rethrow，`:42-45`）。React 侧建议同样不阻塞 shell 渲染，但可静默吞掉。
- 无独立 403 页面、无 404 页面：任何已登录用户都能渲染 shell。

### 1.8 重写要点（易漏）

1. 三态 + **未登录时页面组件不挂载**（等价于「登录后才发请求/才轮询」的数据门禁）。
2. 401 静默登出（无文案）。
3. `≤991px`：侧栏**卸载** + 抽屉成为唯一导航。
4. 单一 `busy` 同时锁提交与退出；单一 `error` 双位置渲染；内容区 Alert 可关闭。
5. AAlert 多行需 `white-space: pre-wrap`。
6. 每次提交后清空密码字段。
7. 退出前检查上传队列（pending/failed）→ 打开抽屉 + 固定文案。
8. 菜单高亮只用 pathname，忽略 search。

---

## 2. `app/pages/dashboard/index.vue`（概览页，59 行）

### 2.1 UI 结构与层级

```
div                                                                                                    // L41
├ DashboardPageHeader(title="概览" description="查看相册、图片和存储状态，快速进入日常管理。")            // L42
│   └ actions：AButton(:loading="isLoading")「刷新」 ; NuxtLink(to="/dashboard/albums") > AButton(type="primary")「管理相册」
└ div.admin-stack                                                                                      // L43
  ├ AAlert(v-if="loadError" type="error" show-icon :message="loadError")                                // L44
  ├ ACard(title="从这里开始")                                                                            // L45
  │   └ div.admin-quick-actions —— **3 张快捷卡片**（`admin.css:54-59`：3 列 grid，≤1100px 变 1 列、gap 12）
  │       ├ NuxtLink /dashboard/albums    : Icon tabler:photo-plus + <strong>添加与整理图片</strong> + <span>创建相册、上传、批量选图</span>
  │       ├ NuxtLink /dashboard/downloads : Icon tabler:file-zip   + <strong>管理公开下载</strong>   + <span>查看 ZIP 状态、批量设置</span>
  │       └ NuxtLink /dashboard/tasks     : Icon tabler:activity   + <strong>查看后台任务</strong>   + <span>生成进度、异常与待确认项</span>
  ├ div.grid.gap-6.sm:grid-cols-3 —— **3 张统计卡**（Tailwind：<640px 单列，≥640px 三列）                // L46-50
  │   ├ ACard > AStatistic(title="相册总数" :value="albums.length")                                      // L47
  │   ├ ACard > AStatistic(title="图片总数" :value="photoCount")                                          // L48
  │   └ ACard > AStatistic(title="当前图片存储" :value="storage ? storageLabels[storage.backend] : '—'"
  │                          :value-style="{ fontSize: 22 }")                                            // L49
  ├ ACard(title="相册")                                                                                  // L51-55
  │   └ ATable                                                                                          // L52
  │       columns（L37）= [ {title:'相册名称', dataIndex:'name'},
  │                         {title:'图片数量', dataIndex:'photoCount', width:120},
  │                         {title:'简介', dataIndex:'description', ellipsis:true},
  │                         {title:'操作', key:'actions', width:160} ]
  │       :data-source="albums" row-key="id" :loading="isLoading"
  │       :pagination="{ pageSize: 8, showSizeChanger: false }" :scroll="{ x: 600 }"
  │       #bodyCell（L53）：
  │         name 列 → NuxtLink { path:'/dashboard/albums', query:{ album: record.id } }  {{record.name}}
  │         actions 列 → ASpace[ NuxtLink「管理图片」→ ?album=id ; NuxtLink「下载设置」→ ?album=id&tab=downloads ]
  └ ACard(title="常用操作")                                                                              // L56
      ├ ASpace(wrap :size="16")：NuxtLink>AButton「管理本地 ZIP」(/dashboard/downloads)、
      │        「存储与缓存维护」(/dashboard/settings/storage)、「修改网站信息」(/dashboard/settings/general)
      └ p.admin-help「图片原件使用当前存储；三层浏览缓存及相册下载 ZIP 始终存放在服务器本地数据目录。」
```

`storageLabels`（L14）：`{ local:'本地存储', webdav:'WebDAV', s3:'S3 对象存储' }`。
页面 meta：`definePageMeta({ layout:'dashboard' })`（L5）、`useHead({ title:'概览' })`（L6）→ 最终标题 `概览 | {siteSettings.title}`（`app.vue:82`）。

### 2.2 API 调用清单（全部 GET，无 query、无 body）

| 调用 | 路径 | 期望响应 |
|---|---|---|
| `adminFetch<Album[]>('/api/albums')`（L21） | `GET /api/albums` | `Album[]`（`app/types/dashboard.ts:11-21`）：`{ id, name, description, createdAt, displayCreatedDate, photoDateStart, photoDateEnd, position, photoCount, coverSource:'auto'\|'photo'\|'upload', coverPhotoId, coverUrl }`。后端排序 `position ASC, created_at DESC, id ASC`（main.rs:3983-3993） |
| `adminFetch<StorageSettings>('/api/settings/storage')`（L22） | `GET /api/settings/storage` | `StorageSettings`（`app/types/dashboard.ts:155-168`）：`{ backend, localPath, webdavUrl, webdavUsername, webdavPrefix, webdavPasswordSet, s3Endpoint, s3Region, s3Bucket, s3AccessKey, s3SecretKeySet, s3Prefix }`。**需管理员**（main.rs:2582-2593 `require_admin`） |

两请求用 `Promise.allSettled` **并行**（L20-23），互不阻塞。

### 2.3 全部交互

- `onMounted(refreshAll)`（L36）；「刷新」按钮 `:loading="isLoading"`（L42）。
- `refreshAll` 有**并发锁**：`if (isLoading.value) return`（L16）。
- 无服务端分页（前端分页 8 条/页，无 pageSize 切换器，`showSizeChanger:false`）；无筛选、无排序控件、无轮询/自动刷新、无删除或编辑动作（动作都在跳转目标页）。
- 表格「简介」列 `ellipsis:true`（超出省略）；`scroll.x=600` 保证窄屏横向滚动。
- 空态：无 `AEmpty`，依赖表格自身空态；`loadError` 存在时**仍渲染**已有（或空）数据。
- 三张统计卡中「当前图片存储」取 `storage.backend` 的**中文标签**（不是原始 backend 值）。

### 2.4 状态管理

| 名称 | 定义 | 含义 |
|---|---|---|
| `albums` | `ref<Album[]>([])` L9 | 相册列表；成功时**整体替换**，失败时保留上次数据 |
| `storage` | `ref<StorageSettings\|null>(null)` L10 | 存储设置，仅用 `backend` |
| `isLoading` | `ref(false)` L11 | 表格 loading + 刷新按钮 loading + 并发锁 |
| `loadError` | `ref('')` L12 | 错误文本（可能多行） |
| `photoCount` | `computed` L13 | `albums.reduce((t,a)=>t+a.photoCount,0)`，**客户端求和，非接口返回值** |
| `columns` | 常量数组 L37 | 列定义 |
| `storageLabels` | 常量对象 L14 | backend → 中文标签 |

### 2.5 错误处理与边界

- `Promise.allSettled` 保证单接口失败不影响另一个；失败项汇总为数组并用 `'\n'` 连接（L29）→ 因 `admin.css:19` 的 `white-space: pre-wrap` 在 Alert 内**多行展示**。文案前缀精确为 `相册：{msg}`、`存储：{msg}`（L26、L28）。
- 外层 `catch`（L30-32）实际只在同步异常时命中（allSettled 不会 reject），写入同一 `loadError`。
- **部分成功语义**：一项成功一项失败时，成功数据照常渲染，失败信息展示在 Alert。
- 空/加载边界：`storage` 为 null → 统计值 `'—'`（em dash）。
- 无重试按钮；重试方式就是再点「刷新」。

### 2.6 通知

**不使用** `useAdminNotice`：全部为内联 `AAlert`（L44）。无 toast、无确认弹窗。

---

## 3. `app/pages/dashboard/tasks.vue`（任务中心，79 行）

### 3.1 UI 结构与层级

```
div                                                                                                     // L63
├ DashboardPageHeader(title="任务中心"
│     description="服务器任务独立运行，不需要停留在此页面。需确认和失败的任务优先显示。")                  // L64
│   └ actions：AButton(:loading="loading")「刷新」 → load()      ← **本页唯一操作按钮**
└ div.admin-stack                                                                                       // L65
  ├ AAlert(v-if="errors.length" type="warning" show-icon                                                // L66
  │        message="部分任务状态暂时无法更新，保留上次结果"          ← 固定文案
  │        :description="errors.join('\n')")                        ← 多行（依赖 pre-wrap）
  ├ ACard(v-if="uploads.state.value.items.length" size="small")  ——「本浏览器的上传队列」                 // L67
  │   └ div.admin-toolbar(style="margin:0")：
  │       左 div[ <strong>本浏览器的上传队列</strong>
  │              + p.admin-help「已入库 {done} · 上传中 {active} · 排队 {queued} · 未确认 {failed}。
  │                             切换后台页面不影响上传，关闭浏览器会停止。」 ]
  │       右 AButton「查看上传队列」 → uploads.open.value = true
  ├ ARadioGroup(v-model:value="filter" option-type="button" :options="filters" aria-label="任务状态筛选")    // L68
  ├ AEmpty(v-if="!filtered.length && !loading" description="这里暂时没有任务")                             // L69
  ├ ACard(v-for="task in filtered" :key="task.id" size="small")   —— 一张卡 = 一个任务                     // L70-75
  │   ├ div.admin-toolbar：<strong>{{task.title}}</strong>
  │   │   └ ASpace：[ ATag(:color = attention?'orange' : active?'processing' : 'default')
  │   │                {{ labels[task.status] || task.status }}
  │   │              , NuxtLink(:to="task.link") {{ task.group==='attention' ? '去处理 →' : '查看详情 →' }} ]  // L71
  │   ├ AProgress(v-if="task.total"
  │   │       :percent="Math.min(100, Math.round(task.completed / task.total * 100))"
  │   │       :status="task.group === 'attention' ? 'exception' : undefined" size="small")                  // L72
  │   ├ div.admin-toolbar(style="margin:4px 0 0")：
  │   │     <span class="admin-help">{{ task.completed }} / {{ task.total }}</span>
  │   │     <span class="admin-help">更新于 {{ new Date(task.updatedAt * 1000)
  │   │            .toLocaleString('zh-CN', { hour12: false }) }}</span>                                    // L73
  │   └ p.admin-field-error(v-if="task.error") {{ task.error }}                                              // L74
  └ p.admin-help「下载任务只展示当前版本；历史 ZIP 在对应相册的"公开下载 → 显示历史记录"中查看。
                  迁移、缓存和 S3 清理展示各自最近一次任务。」                                               // L76
```
页面 meta：`definePageMeta({ layout:'dashboard' })`（L5）、`useHead({ title:'任务中心' })`（L6）。
**没有表单、没有分页、没有排序控件、没有表格**；卡片渲染顺序即排序结果。

### 3.2 轮询：端点 / 间隔 / 停止条件（精确，源码 L42-59）

```ts
42: let timer: ReturnType<typeof setTimeout> | undefined
43: let mounted = false
44: const load = async () => {
45:   if (loading.value) return                      // 重入锁
46:   loading.value = true
47:   const messages: string[] = []
48:   await Promise.all([
49:     adminFetch<AdminAlbumDownloads>('/api/album-downloads')
          .then(value => { downloads.value = value })
          .catch(cause => messages.push(`下载任务：${getAdminApiErrorMessage(cause)}`)),
50:     adminFetch<StorageMigrationJob[]>('/api/storage-migrations')
          .then(value => { migration.value = value[0] || null })
          .catch(cause => messages.push(`迁移任务：${getAdminApiErrorMessage(cause)}`)),
51:     adminFetch<ThumbnailRebuildJob | null>('/api/thumbnails/rebuilds/latest')
          .then(value => { thumbnail.value = value })
          .catch(cause => messages.push(`缓存任务：${getAdminApiErrorMessage(cause)}`)),
52:     adminFetch<S3CleanupJob | null>('/api/s3-cleanups/latest')
          .then(value => { cleanup.value = value })
          .catch(cause => messages.push(`清理任务：${getAdminApiErrorMessage(cause)}`)),
53:   ])
54:   errors.value = messages
55:   loading.value = false
56: }
57: const poll = async () => { await load(); if (mounted) timer = setTimeout(poll, document.hidden ? 15000 : 5000) }
58: onMounted(() => { mounted = true; void poll() })
59: onBeforeUnmount(() => { mounted = false; clearTimeout(timer) })
```

**明确回答：**
- **轮询 4 个端点**（全部 `GET`、无 query、无 body、经 `adminFetch` 即需已登录）：
  1. `GET /api/album-downloads`
  2. `GET /api/storage-migrations`
  3. `GET /api/thumbnails/rebuilds/latest`
  4. `GET /api/s3-cleanups/latest`
- **间隔**：`document.hidden ? 15000 : 5000`（毫秒）。判定发生在**下一轮排期时**（`await load()` 之后）。**没有 `visibilitychange` 监听**：标签页隐藏时仍按 15s 轮询；重新可见后最多再等 15s 才恢复 5s 节奏。
- **首轮**：`onMounted` **立即**执行（不是先等 5s）。
- **停止条件**：**仅** `onBeforeUnmount`（`mounted=false` + `clearTimeout`）。由于无 keep-alive、且未登录时 shell 不渲染页面，因此「离开路由 / 主动登出 / 401 静默登出」都会卸载页面 → 轮询停止。若卸载时 `load()` 仍在飞，其返回后 `if (mounted)` 为 false → 不再排期。
- **递归 `setTimeout`（非 `setInterval`）**：下一轮从上一轮**完成后**计时，慢请求不会堆叠；`loading` 锁进一步防重入。
- 「刷新」按钮**直接调 `load()`**，**不重置定时器**（下一次自动轮询仍按原计划到来）。
- 4 个请求并行，每个都带独立 `.catch` → `Promise.all` **永不 reject**；失败信息进 `errors` 数组，互不影响。
- **部分失败语义**：失败的端点**保留上一次的值**（不清空），顶部显示黄色 Alert + 逐行错误明细。
- `loading` 同时用于按钮 loading 与空态抑制（`AEmpty` 只在 `!filtered.length && !loading` 时显示）。

### 3.3 数据形状（后端精确字段，React 类型照抄）

| 端点 | 响应 |
|---|---|
| `GET /api/album-downloads` | `AdminAlbumDownloads`（`shared/types/downloads.ts:34-39`）= `{ settings: AlbumDownloadSettings[], jobs: AlbumDownloadJob[], localBytes: number, directory: string }`。`directory` 恒为 `"data/album-downloads"`；`localBytes` = `SELECT COALESCE(SUM(byte_size),0) FROM album_download_jobs WHERE status='ready'`；`settings` = **全部相册** LEFT JOIN（未配置的相册也出现，默认 `enabled=false, formats=["webp"], maxImageBytes=5000000, maxZipBytes=0, revision=0`），按 `a.position, a.created_at DESC`；`jobs` 仅最近 **500** 条 `ORDER BY created_at DESC`（`backend/src/album_downloads.rs:237-274`） |
| `GET /api/storage-migrations` | `StorageMigrationJob[]`（`app/types/dashboard.ts:99-116`），最多 **50** 条、`created_at DESC`；**代码只取 `[0] || null`**（tasks.vue:50） |
| `GET /api/thumbnails/rebuilds/latest` | `ThumbnailRebuildJob \| null`（`app/types/dashboard.ts:118-133`）；从未运行时为 JSON 字面 `null`（后端 `fetch_optional` → `Json(Option<_>)`，main.rs:5373-5390） |
| `GET /api/s3-cleanups/latest` | `S3CleanupJob \| null`（`app/types/dashboard.ts:135-153`） |

`AlbumDownloadSettings`（`shared/types/downloads.ts:11-19`）：`{ albumId, albumName, enabled, formats: DownloadFormat[], maxImageBytes, maxZipBytes, revision }`。
`AlbumDownloadJob`（`shared/types/downloads.ts:20-33`）：`{ id, albumId, albumName, format:'png'|'jpg'|'jpeg'|'webp', revision, status, total, completed, byteSize, error:string|null, createdAt, updatedAt }`。

### 3.4 Task 归一化逻辑（核心，逐字移植）

内部类型（L9）：
```ts
type Task = {
  id: string, title: string, status: string,
  group: 'active' | 'attention' | 'finished',
  completed: number, total: number, updatedAt: number,
  error: string | null,
  link: { path: string, query: Record<string, string> }
}
```

**分组函数**（L17，注意精确的状态集合）：
```ts
const groupOf = (status: string): Task['group'] =>
  ['queued','running','deleting'].includes(status) ? 'active'
  : ['failed','interrupted','pending','confirm'].includes(status) ? 'attention'
  : 'finished'
```
即 `ready` / `completed` / `cancelled`（以及下载任务的 `deleted`，已被前置过滤）→ **finished**。

**`tasks` computed**（L18-38），四类来源：

1. **下载 ZIP 任务**（L20-24）
   - 前置过滤（L22）：`const config = downloads.settings.find(s => s.albumId === job.albumId)`；跳过条件 = `!config?.enabled || config.revision !== job.revision || job.status === 'deleted'`。→ **只展示当前 revision 且相册已启用的任务**（历史 ZIP 不出现）。
   - `title = \`${job.albumName} · ${job.format.toUpperCase()} 下载包\``（例：`夏日旅行 · WEBP 下载包`）
   - `link = { path:'/dashboard/albums', query:{ album: job.albumId, tab: 'downloads' } }`
   - `group = groupOf(job.status)`
2. **存储迁移**（L25-30），仅当 `migration` 非 null：
   ```ts
   const cleaning = m.status === 'completed' && m.cleanupStatus === 'cleaning'
   const status = m.status === 'completed' && ['pending','failed','interrupted'].includes(m.cleanupStatus)
                    ? 'confirm' : cleaning ? 'running' : m.status
   title = `存储迁移 · ${m.sourceBackend.toUpperCase()} → ${m.targetBackend.toUpperCase()}${cleaning ? ' · 清理旧副本' : ''}`
   completed = cleaning ? m.cleanupCompleted : m.completed      // 清理阶段改用 cleanupCompleted 计进度
   group = groupOf(status)
   link = { path:'/dashboard/settings/storage', query:{ tab: 'migration' } }
   ```
   `total` 仍取 `m.total`。`confirm` = 「新存储已启用，待管理员决定是否删除旧副本」→ attention。
3. **缩略图 / 三层缓存重建**（L31）：`{ ...thumbnail, title:'三层图片缓存重建', group: groupOf(thumbnail.status), link:{ path:'/dashboard/settings/storage', query:{ tab:'cache' } } }`。
4. **S3 清理**（L32-35）：
   ```ts
   const status = cleanup.status === 'ready' && cleanup.total > 0 ? 'confirm' : cleanup.status
   title = 'S3 旧对象清理'
   link = { path:'/dashboard/settings/storage', query:{ tab:'cleanup' } }
   ```
   `ready` 且候选对象数 >0 → 等待管理员确认删除 → attention。

**排序**（L36-37）：
```ts
const rank = { attention: 0, active: 1, finished: 2 }
return result.sort((a, b) => rank[a.group] - rank[b.group] || b.updatedAt - a.updatedAt)
```
→ **需处理最前，其次进行中，最后已结束；组内按 `updatedAt` 降序**。

**筛选与标签**：
```ts
39: const filtered = computed(() => tasks.value.filter(t => filter.value === 'all' || t.group === filter.value))
40: const filters = computed(() => [
      { label: `全部 ${tasks.value.length}`, value: 'all' },
      { label: `进行中 ${tasks.value.filter(t => t.group === 'active').length}`, value: 'active' },
      { label: `需处理 ${tasks.value.filter(t => t.group === 'attention').length}`, value: 'attention' },
      { label: '已结束', value: 'finished' } ])          // 注意：'已结束' 不带计数
41: const labels: Record<string,string> = {
      queued:'等待运行', running:'运行中', ready:'已就绪', failed:'失败', interrupted:'已中断',
      completed:'已完成', cancelled:'已取消', deleting:'清理中', confirm:'等待确认旧文件处理' }
```
标签渲染回退：`labels[task.status] || task.status`（L71）——未登记的状态直接显示英文原值。

### 3.5 交互与操作按钮（明确回答）

本页**全部可点击元素**：
1. `刷新`（L64）→ `load()`，`:loading="loading"`。
2. 每张任务卡的 `去处理 →` / `查看详情 →`（L71）→ **页内深链跳转**（query 见 §5），不弹窗、不调接口。文案按 group 二分：attention → `去处理 →`，其余 → `查看详情 →`。
3. 上传队列卡的 `查看上传队列`（L67）→ 打开**全局**上传队列抽屉。
4. 顶部筛选 `ARadioGroup`（L68）：纯前端过滤，**不写路由、不写地址栏**；`filter = ref('all')`（L16）为组件本地状态，刷新页面即回到 `all`。

**明确结论：任务中心（tasks.vue）没有「取消 / 恢复 / 重试 / 删除」按钮。** 它是**只读展示 + 深链跳转**页；所有任务操作都在深链目标页（若把操作搬到任务中心，属于行为变更）：

| 任务类型 | 目标页 query | 目标页提供的操作（供校验/参考） | 端点 |
|---|---|---|---|
| 下载 ZIP | `/dashboard/albums?album={albumId}&tab=downloads` | 重新生成；历史记录行「取消」「删除」（带 Popconfirm「删除本地压缩包？原始图片不会删除，可随时重新生成。」） | `POST /api/albums/{id}/downloads/rebuild`；`POST /api/album-downloads/{jobId}/cancel`；`DELETE /api/album-downloads/{jobId}`（`DownloadManager.vue:126,136,189,200-201`） |
| 存储迁移 | `/dashboard/settings/storage?tab=migration` | 安全中断（cancel）、继续迁移（resume）、删除旧存储图片（cleanup，danger）、保留旧副本（retain） | `POST /api/storage-migrations/{id}/{cancel\|resume\|cleanup\|retain}`（`storage.vue:389,556-558`） |
| 三层缓存 | `?tab=cache` | 安全中断（cancel）、清空并重新生成（start）、继续上次任务（resume） | `POST /api/thumbnails/rebuilds/{id}/{cancel\|resume}`、`POST /api/thumbnails/rebuilds`（`storage.vue:276-284,568`） |
| S3 清理 | `?tab=cleanup` | 安全中断（cancel）、扫描旧对象（scan，`disabled: savedBackend!=='s3'`）、确认删除旧对象（delete，条件 `status==='ready' && total>0`）、继续任务（resume） | `POST /api/s3-cleanups/scan`、`POST /api/s3-cleanups/{id}/{delete\|cancel\|resume}`（`storage.vue:328-337,581`） |

按钮显示条件（源站，若在 React 侧同样复刻则用）：
- 迁移 cancel：`['queued','running'].includes(status) || cleanupStatus === 'cleaning'`；resume：`['failed','cancelled','interrupted'].includes(status)`；cleanup/retain：`status === 'completed' && ['pending','failed','interrupted'].includes(cleanupStatus)`。
- 缓存 cancel：`thumbnailTaskActive`；否则 show `清空并重新生成`；resume：`['failed','cancelled','interrupted'].includes(status)`。
- S3 cancel：`s3CleanupActive`；否则 `扫描旧对象`；`确认删除旧对象`：`status==='ready' && total>0`；resume：`['failed','cancelled','interrupted'].includes(status)`。

### 3.6 状态管理（本文件）

| 名称 | 定义 | 说明 |
|---|---|---|
| `downloads` | `ref<AdminAlbumDownloads\|null>(null)` L10 | `/api/album-downloads` 原始响应 |
| `migration` | `ref<StorageMigrationJob\|null>(null)` L11 | 取数组首项 |
| `thumbnail` | `ref<ThumbnailRebuildJob\|null>(null)` L12 | 可为 null |
| `cleanup` | `ref<S3CleanupJob\|null>(null)` L13 | 可为 null |
| `errors` | `ref<string[]>([])` L14 | 4 条端点错误文案，`'\n'` 连接后作为 Alert description |
| `loading` | `ref(false)` L15 | 重入锁 + 按钮 loading + 空态抑制 |
| `filter` | `ref('all')` L16 | 组件本地，切页/刷新重置 |
| `uploads` | `useAdminUploads()` L8 | **全局共享**（`useState('admin-upload-queue')`），只读计数 + 打开抽屉 |
| `timer` / `mounted` | 组件级 `let`（L42-43，**非响应式**） | 轮询控制 |
| `tasks` / `filtered` / `filters` | `computed` L18 / L39 / L40 | 派生数据 |

### 3.7 错误处理与边界

- 每个端点独立 `.catch`，文案前缀精确为：`下载任务：`、`迁移任务：`、`缓存任务：`、`清理任务：`（L49-52），**顺序与 `Promise.all` 数组顺序一致**，`join('\n')` 后逐行显示。
- 失败**不清空**已有数据（保留上次结果），顶部黄色 Alert 固定文案「部分任务状态暂时无法更新，保留上次结果」。
- 401 → `adminFetch` 内 `markUnauthenticated()` → shell 切回登录卡 → 页面卸载 → 轮询停止（不会无限 401 轮询）。
- `adminFetch` 未登录会抛 `Error('请先登录管理员账号')` → 进入 `errors`（理论上不可达，因为未登录时页面不挂载）。
- 空态：`AEmpty description="这里暂时没有任务"`（L69），条件 `!filtered.length && !loading`；筛选到空分组时同样显示。
- 边界：4 个数据源全 null/空 → `tasks` 为空数组 → 显示空态。
- 边界：`task.total` 为 0 或 null → **不渲染进度条**（L72 `v-if="task.total"`），但 `{{completed}} / {{total}}` 仍显示（如 `0 / 0`）。
- 边界：百分比 `Math.min(100, Math.round(completed / total * 100))` —— **上限钳到 100**；`total=0` 因 `v-if` 不会除零。
- 边界：`updatedAt` 单位是**秒**（`*1000` 转毫秒），格式 `new Date(ts).toLocaleString('zh-CN', { hour12: false })`（形如 `2026/2/4 15:07:32`）。**不要**改成 ISO 或第三方库格式。
- 无重试按钮；失败后靠下一轮轮询自动恢复。
- 上传队列卡整卡 `v-if="uploads.state.value.items.length"`：队列为空时整块消失。

### 3.8 通知

**不使用** `useAdminNotice`：全部为内联 `AAlert`（L66，warning）+ `.admin-field-error` 段落（L74，`#cf1322`）。无 toast、无确认弹窗。

---

## 4. `app/composables/useAdminNotice.ts`（14 行，全文）

```ts
1: import { App } from 'ant-design-vue'
2:
3: export function useAdminNotice() {
4:   const { notification, modal } = App.useApp()
5:   return {
6:     add(options: { title: string; description?: string; color?: string }) {
7:       const type = options.color === 'error' ? 'error' : options.color === 'warning' ? 'warning' : options.color === 'success' ? 'success' : 'info'
8:       notification[type]({ message: options.title, description: options.description, placement: 'topRight', duration: type === 'error' ? 8 : 4 })
9:     },
10:    confirm(content: string, danger = false): Promise<boolean> {
11:      return new Promise(resolve => modal.confirm({ title: '确认操作', content, okText: '确认', cancelText: '取消', okButtonProps: { danger }, onOk: () => { resolve(true) }, onCancel: () => { resolve(false) } }))
12:    },
13:  }
14: }
```

精确行为（React 侧必须等价实现）：
- **需要祖先宿主**：`App.useApp()` 从 ant-design-vue 的 App context 取全局 `notification` / `modal` 单例。全仓库唯一的 `<AApp>` 在 `app/layouts/dashboard.vue:53`，所有使用者都在其 slot 内 → 合法。React 侧把 `ToastProvider` / 确认弹窗宿主挂在 **shell 根部**，页面与组件通过 hook/context 调用。
- **调用方共 6 处**：`albums.vue:10`、`UploadQueue.vue:5`、`DownloadManager.vue:7`、`AlbumCoverEditor.vue:8`、`settings/storage.vue:19`（别名 `toast`）、`settings/general.vue:11`（别名 `toast`）。**layout / index.vue / tasks.vue 不使用它**（它们用内联 AAlert）。
- `add()` 类型映射（L7）：`'error'`→error；`'warning'`→warning；`'success'`→success；**其余任何值（含 `undefined`）→ info**。
- 呈现（L8）：右上角（`placement:'topRight'`）；`message`=title、`description`=description；**时长：error = 8 秒，其他（含 info/warning/success）= 4 秒**；多次调用**堆叠多条**（不合并、不替换、无去重）。
- `confirm()`（L10-12）：标题固定 `'确认操作'`；按钮 `确认` / `取消`；`danger=true` → 主按钮危险色；返回 `Promise<boolean>`；**`onOk` 立即 `resolve(true)`，不等待任何异步动作**（调用方必须自己 `await confirm()` 后再发请求）；`onCancel`（点取消、按 ESC）`resolve(false)`。注意该 Promise 只在 onOk/onCancel settle——若宿主被整体卸载导致弹窗销毁，Promise 可能永不 settle（React 侧用受控 Modal 时务必保证 destroy 时 resolve(false)）。
- **不是全局单例 composable**（每次调用返回新对象）；真正的单例是库内部的 notification/modal 实例（应用运行期共享，跨页面不丢失）。
- 无动作按钮、无重试、无关闭回调、无持久化。

---

## 5. 深链 / 「地址栏保留工作区与页签」逻辑（明确回答）

**存在**，且是任务中心与概览页跳转的唯一机制。它不是「地址栏自动同步选中态」，而是**显式路由 query + 目标页读取 query 还原**的双向约定。

### 5.1 写入方（谁产生深链）

| 来源 | 目标 URL | 行号 |
|---|---|---|
| tasks.vue 下载任务卡 | `/dashboard/albums?album={job.albumId}&tab=downloads` | tasks.vue:23 |
| tasks.vue 迁移任务卡 | `/dashboard/settings/storage?tab=migration` | tasks.vue:29 |
| tasks.vue 缓存任务卡 | `/dashboard/settings/storage?tab=cache` | tasks.vue:31 |
| tasks.vue S3 清理卡 | `/dashboard/settings/storage?tab=cleanup` | tasks.vue:34 |
| index.vue 相册名链接 | `/dashboard/albums?album={id}` | index.vue:53 |
| index.vue「管理图片」/「下载设置」 | `?album={id}` / `?album={id}&tab=downloads` | index.vue:53 |
| albums.vue 列表行操作 | `?album=id`、`?album=id&tab=details`、`?album=id&tab=downloads` | albums.vue:253 |
| albums.vue 返回全部相册 | `/dashboard/albums`（**清空 query**） | albums.vue:236、109 |
| UploadQueue 行内相册名 | `/dashboard/albums?album={albumId}` + 关闭抽屉 | UploadQueue.vue:31 |
| DownloadManager | 「管理此相册图片 →」`/dashboard/albums?album={selected}` | DownloadManager.vue:175 |

### 5.2 读取方（谁还原选中态）

**albums.vue（相册工作区）**
```ts
15: const selectedId = computed(() => typeof route.query.album === 'string' ? route.query.album : '')
16: const selectedAlbum = computed(() => albums.value.find(item => item.id === selectedId.value))
17: const tab = computed(() => ['details','downloads'].includes(String(route.query.tab)) ? String(route.query.tab) : 'photos')
18: const listState = useState('admin-album-list-view', () => ({ query:'', page:1, selected: [] as string[] }))
109: const navigateAlbum = (id = '', nextTab = 'photos') => router.push({ path:'/dashboard/albums',
        query: id ? { album: id, ...(nextTab === 'photos' ? {} : { tab: nextTab }) } : {} })
110: const leave = async () => {
       if (locked.value) { notice.add({ title:'正在提交操作，请稍候', color:'warning' }); return false }
       return (!dirty.value && !downloadDirty.value && !orderDirty.value) || await notice.confirm('有未保存的修改，确定放弃修改并离开吗？')
     }
114: onBeforeRouteLeave(leave)
115: onBeforeRouteUpdate((to, from) => to.query.album === from.query.album ? true : leave())
119: watch(selectedId, id => { /* 重置 baseline/ready/photos/selectedPhotos/detailError/orderMode/orderIds/
        downloadDirty/downloadBusy/coverBusy/photoQuery/photoFormat/photoPage，然后 void loadDetail(id) */ })
261: <ATabs :active-key="tab" @change="key => navigateAlbum(selectedId, String(key))">
```

关键语义（React 必须等价）：
- `?album=` 是**工作区开关**：有值 → 进入单相册工作区（标题=相册名、描述=`{n} 张图片 · 在同一个工作区完成图片、资料和下载管理`、面包屑下方 `← 全部相册` + `/ 相册名`、Tab 三项「图片管理（N）/ 相册资料 / 公开下载」）；无值 → 相册列表。
- `?tab=` 白名单 **`details` | `downloads`**，其余（含缺失、`photos`、非法值）一律回落 **`photos`**；且 `tab=photos` 时 URL 中**不写 tab**（L109 条件展开）。
- Tab 切换用 **`router.push`（新增历史条目，浏览器后退可回上一个 Tab）**。
- 切换相册（`?album=` 变化）会**整体重置工作区局部状态**（筛选、页码、未保存标记、排序模式、封面/下载忙碌位），再重新拉详情。
- 守卫：`?album` 未变（例如只切 Tab）→ **直接放行，即使有未保存修改也不弹窗**；`?album` 变化 → 走 `leave()`（locked 时 toast 警告并拒绝；有未保存修改 → 确认弹窗「有未保存的修改，确定放弃修改并离开吗？」）。
- **不在地址栏的状态**：相册列表的搜索词/页码/勾选放 `useState('admin-album-list-view')`（内存共享，**不是 URL**）；图片搜索/格式/排序/页码是本地 ref（切页即丢）；`useState('admin-photo-view')` 的 grid/table 跨页存活。
  → **结论：「地址栏保留工作区 / 页签」只由 `?album=` 与 `?tab=` 两个 query 参数实现**；其余状态靠内存单例或丢弃。**没有** localStorage 记忆、**没有**多页签（tab bar）持久化、**没有**把选中态写回地址栏的 watcher。

**settings/storage.vue（存储与维护）**
```ts
504: const storageRoute = useRoute(); const storageRouter = useRouter()
506: const storageTab = computed({
507:   get: () => ['migration','cache','cleanup'].includes(String(storageRoute.query.tab)) ? String(storageRoute.query.tab) : 'connection',
508:   set: (tab: string) => { void storageRouter.replace({ query: tab === 'connection' ? {} : { tab } }) },
509: })
510: onBeforeRouteLeave(() => !isDirty.value || toast.confirm('存储设置尚未保存，确定放弃修改并离开吗？'))
```
→ 白名单 **`migration` | `cache` | `cleanup`**，其余回落 **`connection`（「存储连接」，且 URL 中移除 tab）**；切换用 **`router.replace`（不产生历史条目）**——与 albums 的 `push` 行为**不同**，必须分别保留。

**其他 query 约定（公共站点，避免误改）**：`?photo=<id>` 控制看图器（`app.vue:37,52-55,67-77`）；`app/router.options.ts:4-13` 的 `scrollBehavior` 在 `to.path === from.path && (to.query.photo || from.query.photo)` 或 photo 路由时返回 `false`（不滚动），否则 `{ top: 0 }`。

### 5.3 参数契约（React 侧照抄）

- 参数名固定：`album`（相册 id）、`tab`。
- `tab` 值集：albums → `photos`(默认，不写) / `details` / `downloads`；storage → `connection`(默认，不写) / `migration` / `cache` / `cleanup`。
- 深链进入是**新的 SPA 导航**；URL 可分享、可刷新（纯前端从 query 还原，不依赖服务端）。
- 因为有 `base: '/dashboard/'`，这些 URL 在 React 侧即 `/dashboard/albums?album=…`，与 Nuxt 现状**逐字一致**；`react-router` 用 `basename="/dashboard"` + `useSearchParams`。

---

## 6. 跨页面共享状态（`useState` key 总表）

| key | 初始值 | 定义位置 | 使用者 | 语义 |
|---|---|---|---|---|
| `chronoframe-admin-auth` | `{checked:false, loading:false, initialized:false, authenticated:false, username:'', error:''}` | `useAdminApi.ts:79` | shell、所有页面（经 `adminFetch`）、UploadQueue | **登录态单例，决定整个管理端渲染分支** |
| `chronoframe-site-settings` | `DEFAULT_SITE_SETTINGS`（title `ChronoFrame`、avatarUrl `/web-app-manifest-192x192.png`、theme `system`） | `useSiteSettings.ts:14`（默认值 `:3-9`） | shell 品牌区/登录卡、`app.vue` titleTemplate | 站点标题、头像、主题 |
| `chronoframe-site-settings-loaded` | `false` | `useSiteSettings.ts:17` | `ensureSiteSettings` | 避免重复请求 |
| `chronoframe-site-settings-loading` | `false` | `useSiteSettings.ts:18` | — | 状态 |
| `chronoframe-site-settings-error` | `''` | `useSiteSettings.ts:19` | — | 错误文案 |
| `admin-upload-queue` | `{ items:[], paused:false, nextId:1, albumVersions:{} }` | `useAdminUploads.ts:7` | shell 顶栏、tasks.vue、albums.vue、UploadQueue | **上传队列本体；跨页面存活，7 并发，成功上传后 `albumVersions[albumId]++`**（`admin-upload-queue.ts:45`，供 albums.vue 局部刷新） |
| `admin-upload-queue-open` | `false` | `useAdminUploads.ts:8` | shell（退出拦截强制打开）、albums.vue、tasks.vue、UploadQueue | 抽屉开关（全局） |
| `admin-album-list-view` | `{ query:'', page:1, selected:[] }` | `albums.vue:18` | albums.vue | 相册列表搜索/页码/勾选，**跨页面保留（含从概览返回）**；改动搜索会重置 page（albums.vue:130） |
| `admin-photo-view` | `'grid'` | `albums.vue:43` | albums.vue | 图片视图网格/列表，跨页面保留 |

`useState` 在 SPA（源 `ssr:false`）中即**应用级内存单例**：路由切换不重置；**浏览器刷新全部重置**。

上传队列行为细节（React 侧重写需要）：并发 **7**（`admin-upload-queue.ts:31`）、**跨相册共享一个控制器**、**永不 abort**（注释 `:25-26`：「失去的响应无法判断服务器是否已提交」）、暂停仅阻止新任务（`:34`，进行中的继续完成）、`retryFailed` 把 failed 且仍有 File 的项重置为 queued（`:62-67`）、`remove` 不允许移除 `uploading`（`:70`）、`clearDone` 过滤 done（`:72`）、Shell 里 `beforeunload` 在 `pending || failed` 时拦截（`UploadQueue.vue:12-14`）。

---

## 7. API 调用总表（本 4 个文件涉及的全部请求）

统一约定：`credentials:'include'`；头 `X-Requested-With: ChronoFrame`；非 GET/HEAD 追加 `X-CSRF-Token`= cookie `cf_csrf` 解码值；`adminFetch` 前置「未登录即抛错」；任意 401 → 静默登出；后端错误体 `{"error": "..."}`。

| 文件 / 触发 | Method | 完整路径 | Body | 期望响应 | 备注 |
|---|---|---|---|---|---|
| shell L47 / L67、useAdminApi 内部 | GET | `/api/auth/status` | — | `{initialized, authenticated, username?}` | 裸 request（不要求已登录）；模块级并发去重（`useAdminApi.ts:39,128-152`） |
| shell L35（注册） | POST | `/api/auth/register` | `{username, password}`（username 已 trim） | `{initialized:true, authenticated:true, username}` + cookies | 409 → 自动 `refreshAuthStatus()` 并 rethrow |
| shell L36（登录） | POST | `/api/auth/login` | 同上 | 同上 | 401 `用户名或密码错误` |
| shell L44（退出） | POST | `/api/auth/logout` | 无 | `{ok:true}` | 403 → refresh 后重试一次；401 视为成功 |
| shell L47 → useSiteSettings | GET | `/api/settings/site` | — | `{title, slogan, author, avatarUrl, theme}` | 公开端点，裸 `fetch` |
| index L21 + 刷新 | GET | `/api/albums` | — | `Album[]` | 与下面一项 `Promise.allSettled` 并行 |
| index L22 + 刷新 | GET | `/api/settings/storage` | — | `StorageSettings` | 需管理员 |
| tasks 轮询（4 个并行） | GET | `/api/album-downloads` | — | `{settings, jobs, localBytes, directory}` | 需管理员；jobs 最近 500 |
| tasks 轮询 | GET | `/api/storage-migrations` | — | `StorageMigrationJob[]` | 取 `[0]` |
| tasks 轮询 | GET | `/api/thumbnails/rebuilds/latest` | — | `ThumbnailRebuildJob \| null` | |
| tasks 轮询 | GET | `/api/s3-cleanups/latest` | — | `S3CleanupJob \| null` | |
| shell 顶栏 UploadQueue（上传） | POST | `/api/albums/{encodeURIComponent(albumId)}/photos` | `FormData{ files: File }` | 不解析响应体 | 并发 7，永不 abort |
| （深链目标页，本 4 文件不直接调用，供校验） | POST | `/api/storage-migrations/{id}/{cancel\|resume\|cleanup\|retain}` | — | — | storage.vue:389 |
| 同上 | POST | `/api/thumbnails/rebuilds`（start）、`/api/thumbnails/rebuilds/{id}/{cancel\|resume}` | — | — | storage.vue:276-284 |
| 同上 | POST | `/api/s3-cleanups/scan`、`/api/s3-cleanups/{id}/{delete\|cancel\|resume}` | — | — | storage.vue:328-337 |
| 同上 | POST / DELETE | `/api/album-downloads/{id}/cancel`、`/api/album-downloads/{id}` | — | — | DownloadManager.vue:136 |
| 同上 | GET | `/api/albums/{id}`（相册详情，含 photos） | — | `AlbumDetail` | albums.vue:87 |

---

## 8. 验收清单（React 重写逐项勾对）

1. 断点是 **991px**（不是 768/1024）；移动端侧栏**被卸载**（不是折叠）；抽屉宽 248、左置；桌面端抽屉存在于 DOM 但不可打开。
2. 侧栏宽 224、sticky、`height:100svh`；顶栏高 64、`line-height:normal`、白底 + `1px #f0f0f0` 下边框；内容区 `max-width:1680px` 居中、padding 24（≤768px 16）；`.admin-stack` gap 24；`.admin-toolbar` gap 12 / margin-bottom 16。
3. 菜单 6 项顺序/文案/tabler 图标名固定，无分组无嵌套；**高亮只按 pathname**，忽略 search；点击关抽屉 + push。
4. 面包屑恒两项（「管理后台 / 页面名」），页面名查表，未登记回退「管理后台」。
5. 三态鉴权（spinner / 登录卡 / shell），**未登录时页面组件不挂载**（数据门禁）。
6. 401 静默登出且无文案；`logout` 的 403→重试语义；`register` 409 → 自动切登录态并清空密码字段。
7. 客户端校验三条中文文案与规则：用户名 1–64 字符、密码 ≥12 字符且 ≤1024 字节、两次输入一致；提交后清空密码/确认密码。
8. 退出前检查上传队列 pending/failed → 打开全局抽屉 + 文案「请先处理上传队列，再退出登录。」+ 不发请求。
9. 概览页：`Promise.allSettled` 部分失败语义、`\n` 多行 Alert（需 pre-wrap）、失败前缀「相册：」「存储：」、`photoCount` 客户端求和、`'—'` 占位、统计卡 <640px 单列、快捷卡 ≤1100px 单列、表格 8 条/页无 size changer + `scroll.x=600` + 简介列 ellipsis + 名称/操作列深链。
10. 任务中心：递归 `setTimeout`，**5s（可见）/ 15s（`document.hidden`）**，仅 unmount 停止，排期时读 `document.hidden`，无 visibilitychange 监听；`loading` 重入锁；`Promise.all` + 逐端点 catch（前缀「下载任务：」「迁移任务：」「缓存任务：」「清理任务：」）；失败保留旧值 + 黄色 Alert 固定文案；`groupOf` 两组状态字符串；排序 attention→active→finished 且组内 `updatedAt` 降序；进度 `min(100, round(c/t*100))`、`total` 为 0 不渲染进度条、attention 时 `exception`；Tag 颜色 orange/processing/default；标签字典 9 项 + 未登记回退英文原值；`更新于` 用 `toLocaleString('zh-CN',{hour12:false})`；筛选默认 `all`、带计数（**「已结束」不带计数**）、不写 URL；空态「这里暂时没有任务」仅非 loading 时；下载任务只显示 `enabled && revision 匹配 && status!=='deleted'`；迁移清理阶段 `completed←cleanupCompleted` 且标题加 ` · 清理旧副本`；S3 `ready && total>0 → confirm` 归 attention；**本页无取消/恢复/重试/删除按钮**；上传队列卡仅在队列非空时出现。
11. 深链：`?album=` / `?tab=`（albums：photos 不落 URL，details/downloads 落 URL，push）/ storage：`migration|cache|cleanup`，默认 `connection` 不落 URL，**replace**；白名单回落；切换相册重置工作区状态并走未保存确认；**只切 Tab 不确认**。
12. 共享单例 8 个 key（§6），跨路由不重置、刷新即重置；上传队列 7 并发/永不 abort/切页续传/`albumVersions` 递增/`beforeunload` 拦截。
13. 通知系统：`add()` 的 color→type 映射（未识别→info）、右上角、**error 8s / 其他 4s**、可堆叠；`confirm()` 固定标题「确认操作」+「确认/取消」+ danger 映射 + **onOk 立即 resolve 不等待请求** + 宿主卸载时也要 settle。
14. 管理后台无 i18n、无暗色、无 keep-alive；`.ant-alert{white-space:pre-wrap}` 的多行能力必须保留；`.admin-field-error` 红色 `#cf1322`、`overflow-wrap:anywhere`。
15. 全部 API 保持相对 `/api/...` 同源请求（`SameSite=Strict` Cookie + `X-Requested-With` + CSRF），路由保留 `/dashboard/...` 且 query 参数名逐字不变。
