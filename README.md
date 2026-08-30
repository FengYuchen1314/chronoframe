# ChronoFrame

面向自托管的个人画廊。项目恢复原作者的 Nuxt 视觉框架，并将服务端重构为：

- 根目录 `app/`、`i18n/`、`shared/`、`public/`：Nuxt 4 + Vue 3 + TypeScript 静态前端
- `backend/`：Rust + Axum + SQLite API
- 管理后台：Ant Design Vue 4，标准侧栏导航、表格、表单、弹窗与任务进度；公开画廊保留原版风格
- 存储：本地磁盘、WebDAV 或 S3 兼容对象存储

公共端恢复原版的相簿动效主页、照片瀑布流、标签/相机/镜头/城市/评分筛选、排序、相簿详情和沉浸式查看器；地图、Globe 和地图管理功能不再存在。默认 `/` 直接展示相簿空间，全图瀑布流位于 `/photos`。上传严格遵循相簿优先的数据规则：管理员必须先创建相簿，之后才能向其中上传图片。管理员还可以重命名或删除相簿、删除单张或多张图片、迁移图片存储位置、维护公开相簿简介和显示日期、调整相簿前后顺序，以及修改网站名称、标语、作者、头像和默认主题。删除相簿时，其中的图片记录和当前存储对象会一并清理；图片被转换或存储迁移任务占用时，后端会拒绝删除以保护数据。

## 单文件部署

提交到 `main` 后，GitHub Actions 会在原生 amd64 和 arm64 runner 上编译 Nuxt/Rust、构建镜像并发布多架构的 `ghcr.io/fengyuchen1314/chronoframe:latest`。部署机器不需要源码、Node.js 或 Rust，只需要 Docker Compose 和根目录的一个 `docker-compose.yml`。

```bash
mkdir chronoframe && cd chronoframe
curl -fsSLO https://raw.githubusercontent.com/FengYuchen1314/chronoframe/main/docker-compose.yml
docker compose up -d
```

也可以手动新建 `docker-compose.yml`，复制下面的完整内容：

```yaml
name: chronoframe

services:
  chronoframe:
    image: ghcr.io/fengyuchen1314/chronoframe:latest
    pull_policy: always
    restart: unless-stopped
    init: true
    stop_grace_period: 30s
    ports:
      - "${CHRONOFRAME_BIND:-0.0.0.0}:${CHRONOFRAME_PORT:-8188}:8080"
    environment:
      CF_DATABASE_URL: sqlite:///app/data/chronoframe.db?mode=rwc
      CF_MASTER_KEY_FILE: /app/data/secret.key
      CF_CONVERSION_WORKERS: "${CF_CONVERSION_WORKERS:-7}"
      CF_COOKIE_SECURE: "${CF_COOKIE_SECURE:-auto}"
      CF_TRUST_PROXY_HEADERS: "${CF_TRUST_PROXY_HEADERS:-true}"
    volumes:
      - ./data:/app/data
```

把文件保存到准备存放相簿数据的目录，在该目录执行 `docker compose up -d`。

默认通过 `0.0.0.0:8188` 提供服务。数据库、主密钥和本地图片会自动写入 Compose 同目录的 `./data`；镜像升级或容器重建不会删除它们。更新只需：

```bash
docker compose pull
docker compose up -d
```

如果旧部署的 Compose 仍使用 `build: .`，`docker compose pull` 不会更新应用。请先改用上方示例中的 `image: ghcr.io/fengyuchen1314/chronoframe:latest`，并保留原来的 `volumes` 映射，再执行更新命令。新版会要求浏览器重新验证入口 HTML，避免更新后因旧页面继续引用已经移除的脚本而出现空白页。

迁移前先停止写入，然后把 Compose 文件和整个 `data` 目录一起打包：

```bash
docker compose stop
sudo tar --numeric-owner -czf chronoframe-backup.tgz docker-compose.yml data
docker compose start
```

主密钥权限固定为 `0600`，因此打包必须使用 `sudo` 并确认命令成功；恢复时同样使用 `sudo tar --numeric-owner -xzf chronoframe-backup.tgz`。在新机器解压后进入目录执行 `docker compose up -d` 即可。若使用 WebDAV 或 S3，目录备份会保留数据库、管理员数据、加密主密钥及连接配置，远端图片本身仍在原 WebDAV/S3 中；需要离线完整迁移时还必须另行迁移远端对象。

Compose 无需 `.env`。默认同时允许通过公网 `IP:8188`、域名和 HTTP/HTTPS 反向代理访问；管理请求不会因为 Origin、反代协议、域名或端口不同而被拒绝。程序默认信任 `Forwarded` / `X-Forwarded-Proto` 来自动决定 Cookie 是否增加 `Secure`，代理没有发送这些头时仍可正常使用。需要修改监听地址、端口或 Cookie 策略时，才需使用 `.env.example` 中的可选变量。

全新数据库第一次打开 `/dashboard` 时会显示管理员注册页。第一笔合法注册会在同一个 SQLite 事务中创建管理员和初始会话；一旦创建成功，注册入口永久关闭，之后只能使用该用户名和密码登录。不要让尚未完成首次注册的实例长期暴露在公网，否则其他访问者可能先行取得管理员身份。默认配置可直接使用 HTTP `IP:端口`，也可放在常见的 HTTPS 反向代理后面，无需额外切换环境变量。

## 存储后端

所有存储连接参数都在管理员界面的“存储设置”中维护，并持久化到 SQLite；程序不会从 `.env` 读取本地路径、WebDAV 或 S3 参数。后台可先执行完整的写入、读回、删除连接测试，再保存设置。没有图片时可直接切换目标；已有图片时，保存新的类型、路径、Endpoint、桶或前缀会进入安全迁移流程。同一目标的凭据轮换仍然允许。

单文件 Docker 部署使用本地存储时，请保持后台默认路径 `./data/storage`；它对应宿主机当前目录的 `./data/storage`。改到 `/app/data` 之外的容器路径不会被 Compose 持久化，也不会进入上述目录备份。

Compose 变量只负责服务运行时，不负责存储：

- `CHRONOFRAME_BIND`、`CHRONOFRAME_PORT`：宿主机监听地址和端口。
- `CF_COOKIE_SECURE`：`auto`、`true` 或 `false`；默认 `auto`，反代报告 HTTPS 时自动使用 Secure Cookie。
- `CF_TRUST_PROXY_HEADERS`：是否信任 `Forwarded` 和 `X-Forwarded-Proto`；默认 `true`，以兼容无需额外配置的反向代理部署。
- `CF_CONVERSION_WORKERS`：仅用于兼容旧版本遗留转换任务，默认 7，限制为 1–16；新上传的三层派生图会自动按 CPU 调整并发。

容器内部的 SQLite 路径、主密钥路径、静态前端目录和监听地址已固定在镜像与 Compose 中，无需用户配置。

WebDAV 密码和 S3 秘密访问密钥使用独立安装主密钥进行 AES-256-GCM 加密，读取设置时永不返回明文。主密钥与管理员密码完全解耦；备份或迁移时必须同时保存 SQLite 数据库和 `CF_MASTER_KEY_FILE`，缺少任意一项都无法恢复存储凭据。上传对象在 WebDAV 中会先写入临时对象，再使用 `MOVE` 原子提交；S3 使用临时对象复制到最终键；本地存储则先写入同目录临时文件后重命名。请使用支持 WebDAV `MKCOL`、`PUT`、`MOVE`、`DELETE` 的服务端，以及兼容 S3 path-style 请求的对象存储服务。

从旧的 `X-Admin-Token` 版本升级时，可在第一次启动新版时暂时保留原 `CF_ADMIN_TOKEN`。只有在主密钥文件尚不存在时，程序才会一次性用旧令牌派生兼容密钥并写入 `CF_MASTER_KEY_FILE`；确认密钥文件已经生成后即可移除该环境变量。它不会再被用于登录或 API 鉴权。

上传必须指定一个已存在的相簿。应用本身不限制单次选择的图片数量、单张大小或总大小；浏览器默认使用 7 个异步 worker 连续提交，Rust 后端同时允许 7 个上传请求并行处理，避免逐张等待产生的带宽空档。每张图片仍独立提交并确认，某张失败不会影响其他成功项，失败文件会保留在待上传列表中供直接重试。图片入库后会立即进入并发派生图队列，不需要管理员再执行格式转换。服务仍会校验扩展名、文件签名和完整解码，只接受 PNG、JPG/JPEG 与 WEBP。若通过第三方反向代理或 CDN 访问，还需确保其请求体大小、连接数和超时配置不会额外限制上传。

## 图片删除与存储迁移

管理员可在相簿工作区进入“管理图片”，单选、全选或多选图片后永久删除。数据库记录与持久化删除 outbox 在同一个事务中提交；即使 S3/R2、WebDAV 或本地对象删除临时失败，图片也不会重新出现在相簿中，后台会继续重试尚未清理的对象。

存储中心支持在本地、WebDAV、S3/R2 之间迁移全部现有图片：

- 迁移以后台并发任务运行，公开相簿在复制期间仍可查看；为保证快照一致，上传、转换、删除等写操作会暂时拒绝并提示稍后重试。
- 每个目标对象写入后都会重新读回，并核对字节数与 SHA-256。只有全部对象校验成功，系统才会在一个数据库事务中切换唯一活动存储。
- 复制失败、管理员中断或服务重启都不会切换存储。后台保留逐项进度，选择“继续迁移”时只重跑未成功的对象；已经写完但尚未来得及记录进度的对象也会先校验再复用。
- 切换后旧存储默认完整保留。管理员必须明确选择“删除旧存储图片”或“保留旧存储”；选择删除时，系统会再次验证当前存储中的副本，再通过可重放任务删除旧对象。

迁移本地存储时，目标路径必须位于 Compose 持久化卷内，例如 `/app/data/storage-new`；如果填写 `/app/data` 之外的路径，容器重建后该位置不会保留。

当前活动存储为 S3/R2 时，存储中心还提供“旧空间回收”：系统只分页扫描当前配置前缀下的 `albums/` 对象，不会触碰同一桶里的其他目录。数据库仍引用的母本、上传暂存账本、图片删除 outbox 和旧格式删除 outbox 会组成保护集合；最近 24 小时的对象也会进入宽限期。扫描结果只展示孤儿对象数量和预计可释放容量，不会自动删除；管理员二次确认后才以 8 并发后台清理，并在每次删除前重新核对保护集合。任务可离开页面、安全中断、继续，服务重启会标记为可恢复的中断状态。S3 凭据除读写对象外还需要 `ListBucket`（列出对象）和删除对象权限。

## 相簿打包下载与站点自定义

### 公开相册下载

进入后台 **下载管理**，选择相册，开启「可供下载」，选择 PNG、JPG、JPEG、WebP 中的一种或多种，并设置**单张图片大小上限（MB）**，保存即可。默认每张最多 5 MB，填写 `0` 表示不限；MB 按 1,000,000 字节计算。此限制不是整个 ZIP 的大小，也不会因为超限而漏掉某张图片：JPEG/WebP 会调整编码质量，仍超限时缩小分辨率；PNG 保持无损编码、通过缩小分辨率满足限制。透明图片转 JPEG 使用白色背景。

- 每种格式生成独立 ZIP，内容从存储母本生成，不使用低清缩略图。JPG 与 JPEG 编码相同，扩展名不同。
- 相册首页每张卡片右上角、相册详情标题旁都会显示下载按钮。单格式直接下载，多格式展开下拉菜单；未生成完的格式显示「正在打包」。未开启的相册不显示按钮。
- ZIP 始终保存在服务器本地的 **`./data/album-downloads`**，不上传 S3/R2 或 WebDAV。使用上面的 Compose 时，该目录随 `./data` 持久化、备份和迁移，不需要新增环境变量或挂载。
- 任务在服务器后台执行，离开后台不会停止。最多同时处理 2 个 ZIP、全局 4 路图片编码；管理页显示进度、失败原因和已发布 ZIP 占用。
- 增删图片、修改名称或下载设置后自动生成新版本。旧版本立即停止公开下载并异步清理，只有完整写入并原子提交的当前版本会发布。
- 管理员可以取消生成或删除本地 ZIP。删除后不会立刻自动生成同一版本；点击「重新生成」即可恢复。关闭「可供下载」会撤下链接并清理该相册本地 ZIP，**不会删除原图或远端存储对象**。
- 服务重启会清理未完成临时文件并重跑未完成任务；取消、删除状态持久化，过期任务不能重新发布。磁盘余量不足时停止打包，预留 256 MiB 安全空间；失败文件会清理，释放空间后可重新生成。

旧版本升级后所有相册默认关闭公开下载，需要管理员按相册开启。启用后无需登录即可下载，适合公开分享；下载不会绕过已关闭或已删除的状态。

### 管理员导出与站点信息

管理员可在“相簿”页独立勾选一个或多个相簿并下载：只选一个时，ZIP 内直接放置该相簿的图片；选择多个时，下载一个外层 ZIP，每个所选相簿在其中对应一个独立 ZIP。重名文件和重名相簿会自动追加序号，文件名会过滤路径分隔符和不安全字符。

打包同时支持本地、WebDAV 和 S3/R2 存储。服务逐张读取对象并写入磁盘临时区，再由受限线程池压缩并流式发送最终文件，不会把整个相簿压缩包保存在内存中；下载完成、浏览器中断或生成失败都会清理临时目录。同一时刻最多生成两个相簿导出，避免大型相簿把 CPU、磁盘和远端存储连接占满。

“站点设置”恢复原版的公开自定义项：网站名称、标语、作者、头像 URL 和默认浅色/深色/跟随系统主题。它们与存储配置一样保存在 SQLite 中，不需要环境变量；地图、统计脚本、第三方登录等已删除功能不会因此恢复。

## 三层图片、按需导出与重建任务

相册瀑布流按数据库中已知的图片宽高一次计算列位置，不再逐张插入 DOM 测量；只有视口及上下约 320px 内的卡片开始请求 PNG 缩略图。直接滚动到相册底部时，底部图片独立加载，不会排在未浏览图片之后。这是按可见区域调度请求，不是带宽限速。

查看器以相册页上的覆盖层打开（链接形如 `/albums/相册ID?photo=图片ID`），不会卸载相册、在背后渲染全站图库或在关闭时重新请求相册。原来的 `/图片ID` 链接仍可用，只查询该图的元数据后定位到所属相册。关闭动画只执行一次 240ms 的位移与缩放，支持浏览器返回、单击关闭、手机下滑关闭和减少动态效果设置；动画完成回调有 320ms 超时兜底，页面已在后台时直接跳过动画，避免浏览器暂停动画导致退出一直等待。

当前图片独立加载，之后仅预加载前后各两张预览图，最多同时两个低优先级请求。切图时保留并提升正在加载的新当前图，不再全部取消重来。桌面和手机只挂载各自的查看器；桌面缩略图条按可见窗口渲染，缓存命中的图片无需再次等待固定渐显。调度回归测试可在 VPS 的 Node 24 环境执行 `node --test scripts/viewer-performance.test.mjs`。

原始上传文件继续作为存储母本保存在本地、WebDAV 或 S3/R2，公开页面不会直接加载它。应用在 Compose 同目录的 `./data/thumbnails` 为每张图片维护三层派生图：

- 相簿网格使用最长边 320px 的低清 PNG，优先让页面快速铺满。
- 点进查看器默认使用最长边不超过 2560px、文件严格不超过 1.5 MB 的 WebP；当前图加载完成后才预取左右邻图的同层版本。
- 只有点击查看器底部“显示高清”后，当前图片才加载最长边不超过 4096px、文件严格不超过 5 MB 的 WebP。不会预取整本相簿的高清版本。

上传接口不等待派生图编码完成；每张图片写入数据库后立即进入 7–32 路自适应并发队列。若后台处理尚未完成，三个公开接口也会按需生成缺失层。管理员可在“存储设置”一键清空并重建全站三层缓存，兼容旧版本上传的图片；该任务逐张持久化进度，可离开页面、安全中断、继续，并在服务重启后自动恢复。

在图片上单击右键，或在移动设备上长按，可展开“复制为”和“下载为”，支持 WEBP、PNG、JPG/JPEG。非 WebP 格式由服务从 5 MB 高清层现场转换，不会新增相簿图片，也不会改动原始母本。浏览器的图片剪贴板只在安全上下文中开放，因此“复制为”需要 HTTPS；HTTP IP + 端口访问仍可浏览和下载。

相簿网格支持多选：桌面端可点击勾选并用 Shift 连选，下载时生成一个 ZIP；移动端长按进入多选后按顺序逐张下载，并显示完成进度。旧版“批量格式转换”入口已经从后台移除；升级时遗留的后端任务表和接口暂时保留，仅用于兼容已有数据。

管理员密码使用带随机盐的 Argon2id 哈希保存，不会写入 Cookie 或前端存储。登录成功后服务端签发 7 天有效的随机会话：浏览器只持有 `HttpOnly`、`SameSite=Strict` 的会话 Cookie，数据库只保存令牌摘要；管理写操作还必须通过与该会话绑定的 CSRF 双重校验。退出会立即删除服务端会话，过期会话不能重放。项目是前后端同源应用，不开放宽松 CORS。

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

## 验收测试

`scripts/vps-e2e.sh` 会在隔离的 Docker Compose 项目中拉取 `CHRONOFRAME_IMAGE` 指定的 Actions 镜像，覆盖并发首次注册、Argon2id 哈希、Cookie/CSRF、会话过期与退出，以及本地、WebDAV、S3、四种格式互转、多相簿、并行任务、取消、并发读写、硬终止恢复和临时对象清理；`scripts/vps-s3-cleanup-e2e.sh` 使用隔离 MinIO 验证 24 小时宽限、管理前缀隔离、删除前引用保护和孤儿对象清理；`scripts/vps-delete-interrupt.sh` 专门验证登录会话和管理员确认删除后的 outbox 在进程被强制终止时能够安全续作。`scripts/vps-load.py` 用于并发混合负载和延迟阈值检查。

本项目基于原项目的 MIT 许可继续发布，原作者为 HoshinoSuzumi / Timothy Yin。
