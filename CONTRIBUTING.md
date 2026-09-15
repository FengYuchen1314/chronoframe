# Contributing

Open Gallery 由 Nuxt 4 + Vue 3 + TypeScript 公开画廊、React 管理后台和 Rust 后端组成。两个前端生成静态文件，由 Rust 服务统一托管。

## 项目结构

- `app/`、`i18n/`、`shared/`、`public/`：公开画廊，Nuxt 4 + Vue 3 + TypeScript 静态前端
- `admin/`：管理后台，独立构建的 React 19 + HeroUI v3 + Tailwind CSS v4 + React Router 7 单页应用
- `backend/`：Rust + Axum + SQLite API 与后台任务
- `scripts/`：前端逻辑回归测试、后台构建脚本与 VPS 验收脚本

管理后台与公开画廊是**两套独立构建**。前台用 `@nuxt/ui`，后台用 HeroUI，两者都基于 Tailwind v4，放在同一个构建里会互相渗透全局样式（preflight、主题变量、工具类），因此后台拥有自己的 `package.json`、lockfile 与 Vite 配置，产物落到 `public/dashboard`，再随 Nuxt 静态产物一起发布到 `/dashboard/`。改造后台不会影响公开画廊的任何一个文件。

## 开发约定

源码可以在本地编辑；提交到 `main` 后，由 GitHub Actions 在 amd64/arm64 runner 上完成正式容器编译和发布。不要在本地工作区运行 `pnpm build`、`cargo test` 或 Docker 构建。

需要排查编译问题时，前端检查命令为：

```bash
corepack enable
corepack prepare pnpm@10.34.1 --activate
pnpm install --frozen-lockfile
pnpm build
```

Rust 检查命令为：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

## 管理后台开发

```bash
pnpm dev:admin       # 开发管理后台（Vite dev server，:5174，/api 代理到 :8080）
pnpm dev             # 开发公开画廊（Nuxt dev server）
pnpm typecheck:admin # 管理后台类型检查
pnpm typecheck       # 公开画廊类型检查
pnpm build:admin     # 只构建后台
pnpm build:web       # 只构建公开画廊
pnpm build           # 先构建后台，再运行 nuxt generate（发布用）
```

`pnpm build` 会先运行 `scripts/build-admin.mjs`：按需在 `admin/` 内安装依赖、构建 SPA，并把产物复制到 `public/dashboard`。随后 `nuxt generate` 把 `public/` 原样复制进 `.output/public`，后台就出现在 `/dashboard/` 下。`admin/dist`、`public/dashboard` 都是构建产物，不入库。

> 调试后台请用 `pnpm dev:admin`（Vite 热更新，无需构建）。若确实需要查看构建产物，必须运行完整的 `pnpm build`：服务端提供的是 `.output/public/dashboard`，而 `pnpm build:admin` 只更新到 `public/dashboard` 为止，不会触发 `nuxt generate`，刷新页面拿到的仍是上一次的旧代码。

后台代码位于 `admin/src/`：`lib/`（API 客户端、类型、格式化、上传队列、主题、跨页共享状态）、`components/`（外壳、表格、分页、上传队列抽屉、下载管理、封面编辑器、图片预览）与 `pages/`（概览、相册管理、下载管理、任务中心、站点设置、存储与维护）。`admin/docs/` 下的四份规格记录了重写前 Vue 版后台的逐行行为，是后台行为对齐的依据。

改动后台时须留意两处服务端配合：

- `backend/src/main.rs` 为 `/dashboard` 挂了独立的静态服务，用后台自己的 `index.html` 做深链回退。后台深链（如 `/dashboard/albums?album=xxx`）在 `ServeDir` 里没有对应文件，若不单独回退就会落到公开画廊的 Nuxt 外壳。`/dashboard/assets` 另行挂载且**不带回退**，缺失的资源必须返回 404。
- 同一文件的缓存策略把 `/dashboard/assets/` 视为带内容哈希的不可变资源，与 `/_nuxt/` 一样给一年 `immutable` 缓存，且不会给 HTML 打这个头；HTML 始终 `no-cache`，避免发版后命中旧外壳。

**地址栏状态约定（对齐现网行为，不要改动）**：`/dashboard` 的路径与 `?album=`、`?tab=` 查询参数是地址栏唯一的持久状态。相册页的 `tab` 只认 `details` 与 `downloads`（默认 `photos` 且不写进 URL），切换用 push；存储页的 `tab` 只认 `migration`、`cache`、`cleanup`（默认 `connection` 且不写进 URL），切换用 replace。其余列表状态（搜索词、页码、勾选、网格/列表视图）只存在内存里，刷新即重置。

## 改名说明

项目由 ChronoFrame 更名为 Open Gallery。为了不影响已有部署，以下运行时标识**有意保留旧名**，不要顺手改掉：

- 请求头 `X-Requested-With: ChronoFrame`：后端在注册、登录、退出接口上强制校验，值不符即返回 403
- 环境变量前缀 `CF_` 与 `CHRONOFRAME_BIND` / `CHRONOFRAME_PORT` / `CHRONOFRAME_IMAGE`
- 数据库文件 `chronoframe.db`、Compose 项目名与服务名 `chronoframe`、容器内用户 `chronoframe`
- WebDAV / S3 默认存储前缀 `chronoframe`：已有对象就存放在这个前缀下
- 浏览器存储键 `cframe-color-mode`、`chronoframe-locale` 等：改掉会重置访客偏好

## 测试

前端逻辑回归：

```bash
node --experimental-strip-types --test scripts/*.test.mjs
```

覆盖上传队列的 7 并发、暂停与重试、状态通知时机、相册目标固定、日期校验、跨页多选、存储页进度与状态映射，以及公开查看器、封面和下载逻辑。请使用与镜像一致的 Node 24 环境，勿混用不同系统的原生依赖目录。

Actions 镜像发布后，涉及存储、上传、转换或中断语义的修改还要在 VPS 的隔离 Compose 项目中运行：

```bash
bash scripts/vps-e2e.sh
bash scripts/vps-delete-interrupt.sh
```

通过 `CHRONOFRAME_IMAGE=ghcr.io/uniseem/open-gallery:sha-<commit>` 指定不可变镜像。验收脚本会创建并清理自己的 Docker Compose 测试数据；必须使用独立的 `PROJECT_NAME` 和端口，不要把真实凭据、个人照片或生产数据库放进测试目录。

- `scripts/vps-e2e.sh`：并发首次注册、Argon2id 哈希、Cookie/CSRF、会话过期与退出，以及本地、WebDAV、S3、多相簿、并行任务、取消、并发读写、硬终止恢复和临时对象清理
- `scripts/vps-s3-cleanup-e2e.sh`：使用隔离 MinIO 验证 24 小时宽限、管理前缀隔离、删除前引用保护和孤儿对象清理
- `scripts/vps-delete-interrupt.sh`：登录会话和管理员确认删除后的 outbox 在进程被强制终止时能够安全续作
- `scripts/vps-load.py`：并发混合负载与延迟阈值检查

## API 摘要

- `GET /api/auth/status` — 查询是否已完成首次注册及当前会话状态
- `POST /api/auth/register` — 仅在无管理员时原子创建第一个管理员并登录
- `POST /api/auth/login`、`POST /api/auth/logout`
- `GET/PUT /api/settings/storage` — 管理员读取或保存存储后端设置
- `POST /api/settings/storage/test` — 在不保存的情况下测试候选存储
- `GET/PUT /api/settings/site` — 公开读取站点信息，或由管理员保存站点自定义设置
- `GET/POST /api/albums`
- `POST /api/albums/order` — 提交包含全部当前相簿 ID 的新顺序
- `GET /api/albums/export?albumIds=id1,id2` — 管理员流式下载单相簿 ZIP 或多相簿嵌套 ZIP
- `GET /api/album-downloads/public` — 公开相册各格式下载状态与链接
- `GET /api/album-downloads` — 管理员查询下载设置、任务进度与本地占用
- `PUT /api/albums/:album_id/download-settings` — 设置 `{ "enabled": true, "formats": ["png", "webp"], "maxImageBytes": 5000000 }`
- `POST /api/albums/:album_id/downloads/rebuild` — 后台重新生成相册的所有已选格式
- `GET /api/albums/:album_id/downloads/:format` — 公开流式下载当前 ZIP，支持 Range 断点续传
- `GET /api/albums/:album_id/downloads/:format/photos` — 当前已发布 ZIP 的图片清单
- `GET /api/albums/:album_id/downloads/:format/photos/:index?version=job_id` — 按清单逐张下载，支持 Range，每次请求均检查相册仍可下载且版本匹配
- `PUT /api/album-downloads/settings/bulk` — 管理员批量覆盖下载设置；请求为 `{ "target": { "scope": "selected", "albumIds": ["id1", "id2"] }, "settings": { "enabled": true, "formats": ["png", "webp"], "maxImageBytes": 5000000 } }`，全部现有相册使用 `"target": { "scope": "all" }`；未知相册或无效设置会使整批操作失败，不会部分保存
- `POST /api/album-downloads/:job_id/cancel`、`DELETE /api/album-downloads/:job_id` — 管理员取消任务或删除本地包，不删除原图
- `GET/PATCH /api/albums/:album_id` — 相簿详情及其中的图片，或修改简介和显示日期
- `GET/POST /api/albums/:album_id/photos`
- `GET /api/photos` — 按创建时间倒序列出图片
- `DELETE /api/photos/:photo_id`、`POST /api/photos/delete` — 删除单张或批量删除图片
- `GET /api/photos/:photo_id/thumbnail` — 320px PNG 网格图
- `GET /api/photos/:photo_id/preview`、`GET /api/photos/:photo_id/high` — 1.5 MB 默认查看图和 5 MB 手动高清图
- `GET /api/photos/:photo_id/render?format=webp|png|jpg|jpeg&download=true` — 按需复制或下载指定格式
- `POST /api/photos/export` — 提交 `{ "photoIds": [], "format": "webp|png|jpg|jpeg" }` 并流式下载多选 ZIP
- `GET/POST /api/storage-migrations` — 查看迁移进度或以新的存储配置开始迁移
- `POST /api/storage-migrations/:job_id/resume`、`POST /api/storage-migrations/:job_id/cancel`
- `POST /api/storage-migrations/:job_id/cleanup`、`POST /api/storage-migrations/:job_id/retain` — 删除或保留旧存储图片
- `GET /api/s3-cleanups/latest`、`POST /api/s3-cleanups/scan` — 查看最近任务或扫描当前 S3 管理前缀中的孤儿对象
- `POST /api/s3-cleanups/:job_id/delete|resume|cancel` — 确认后台清理、继续或安全中断 S3 旧空间任务
- `GET /api/thumbnails/rebuilds/latest`、`POST /api/thumbnails/rebuilds` — 查看最近任务或清空缓存并开始并发重建三层派生图
- `POST /api/thumbnails/rebuilds/:job_id/resume`、`POST /api/thumbnails/rebuilds/:job_id/cancel` — 继续或安全中断派生图重建

## 产品约束

- 公共界面沿用原作者的 Nuxt 画廊布局、相簿分类、筛选器和图片查看器；地图与 Globe 功能不再提供。
- 上传入口必须属于已存在的相簿，不能创建无相簿图片。
- 只接受 PNG、JPG/JPEG、WEBP；扩展名、文件签名和完整解码都必须一致。
- 写入必须先进入临时对象，再原子提交；任何新增崩溃窗口都要进入持久化恢复账本。
- 格式转换必须使用受限 worker，不能阻塞上传、读取或 HTTP 运行时。
- 转换完成后默认保留原图；只有管理员明确确认后才可进入持久化删除 outbox。
- 本地磁盘、WebDAV 与 S3 参数只能在后台「存储设置」中管理；密钥不得由读取 API 返回明文，也不得写入日志。

提交信息建议遵循 Conventional Commits。Pull Request 请说明数据安全影响，并附上 VPS 测试结果。

## 贡献者

原项目 [ChronoFrame](https://github.com/HoshinoSuzumi/chronoframe) 由 Timothy Yin（[HoshinoSuzumi](https://github.com/HoshinoSuzumi)）创建。本仓库保留完整 git 历史，其中原作者与第三方贡献者提交的署名均不作改动。

二次开发与重构部分由 [Uniseem](https://github.com/Uniseem) 维护并对代码负责，实现过程借助 AI 编程助手完成：

| 时间 | 协作者 | 范围 |
|---|---|---|
| 2026-08-23 ～ 09-01 | **GPT** | 服务端改写为 Rust、相簿优先的数据模型、三层派生图与查看器性能、存储迁移与 S3 旧对象清理、公开下载与批量设置、Ant Design 后台 |
| 2026-09-12 ～ 09-13 | **DeepSeek**、**Claude** | 管理后台重写为 `admin/` 下的独立 React SPA。DeepSeek 完成重写主体与四份实现规格，Claude 完成 review、缺陷修复与回归测试 |
