# ChronoFrame

面向自托管的个人画廊。项目恢复原作者的 Nuxt 视觉框架，并将服务端重构为：

- 根目录 `app/`、`i18n/`、`shared/`、`public/`：Nuxt 4 + Vue 3 + TypeScript 静态前端
- `backend/`：Rust + Axum + SQLite API
- 存储：本地磁盘、WebDAV 或 S3 兼容对象存储

公共端恢复原版的相簿动效主页、照片瀑布流、标签/相机/镜头/城市/评分筛选、排序、相簿详情和沉浸式查看器；地图、Globe 和地图管理功能不再存在。默认 `/` 直接展示相簿空间，全图瀑布流位于 `/photos`。上传严格遵循相簿优先的数据规则：管理员必须先创建相簿，之后才能向其中上传图片。管理员还可以删除单张或多张图片、迁移图片存储位置、维护公开相簿简介和显示日期、调整相簿前后顺序，以及修改网站名称、标语、作者、头像和默认主题。

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
      CF_CONVERSION_WORKERS: "${CF_CONVERSION_WORKERS:-4}"
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
- `CF_CONVERSION_WORKERS`：全局转换 worker 上限，限制为 1–16。

容器内部的 SQLite 路径、主密钥路径、静态前端目录和监听地址已固定在镜像与 Compose 中，无需用户配置。

WebDAV 密码和 S3 秘密访问密钥使用独立安装主密钥进行 AES-256-GCM 加密，读取设置时永不返回明文。主密钥与管理员密码完全解耦；备份或迁移时必须同时保存 SQLite 数据库和 `CF_MASTER_KEY_FILE`，缺少任意一项都无法恢复存储凭据。上传和转换在 WebDAV 中会先写入临时对象，再使用 `MOVE` 原子提交；S3 使用临时对象复制到最终键；本地存储则先写入同目录临时文件后重命名。请使用支持 WebDAV `MKCOL`、`PUT`、`MOVE`、`DELETE` 的服务端，以及兼容 S3 path-style 请求的对象存储服务。

从旧的 `X-Admin-Token` 版本升级时，可在第一次启动新版时暂时保留原 `CF_ADMIN_TOKEN`。只有在主密钥文件尚不存在时，程序才会一次性用旧令牌派生兼容密钥并写入 `CF_MASTER_KEY_FILE`；确认密钥文件已经生成后即可移除该环境变量。它不会再被用于登录或 API 鉴权。

上传必须指定一个已存在的相簿。单次请求最多 128 张、总计 384 MiB、单张 100 MiB；服务会在写入任何对象前校验整批文件的扩展名、文件签名和完整解码，批次中任一文件无效时不会留下部分上传。

## 图片删除与存储迁移

管理员可在相簿工作区进入“管理图片”，单选、全选或多选图片后永久删除。数据库记录与持久化删除 outbox 在同一个事务中提交；即使 S3/R2、WebDAV 或本地对象删除临时失败，图片也不会重新出现在相簿中，后台会继续重试尚未清理的对象。

存储中心支持在本地、WebDAV、S3/R2 之间迁移全部现有图片：

- 迁移以后台并发任务运行，公开相簿在复制期间仍可查看；为保证快照一致，上传、转换、删除等写操作会暂时拒绝并提示稍后重试。
- 每个目标对象写入后都会重新读回，并核对字节数与 SHA-256。只有全部对象校验成功，系统才会在一个数据库事务中切换唯一活动存储。
- 复制失败、管理员中断或服务重启都不会切换存储。后台保留逐项进度，选择“继续迁移”时只重跑未成功的对象；已经写完但尚未来得及记录进度的对象也会先校验再复用。
- 切换后旧存储默认完整保留。管理员必须明确选择“删除旧存储图片”或“保留旧存储”；选择删除时，系统会再次验证当前存储中的副本，再通过可重放任务删除旧对象。

迁移本地存储时，目标路径必须位于 Compose 持久化卷内，例如 `/app/data/storage-new`；如果填写 `/app/data` 之外的路径，容器重建后该位置不会保留。

## 相簿打包下载与站点自定义

管理员可在“相簿”页独立勾选一个或多个相簿并下载：只选一个时，ZIP 内直接放置该相簿的图片；选择多个时，下载一个外层 ZIP，每个所选相簿在其中对应一个独立 ZIP。重名文件和重名相簿会自动追加序号，文件名会过滤路径分隔符和不安全字符。

打包同时支持本地、WebDAV 和 S3/R2 存储。服务逐张读取对象并写入磁盘临时区，再由受限线程池压缩并流式发送最终文件，不会把整个相簿压缩包保存在内存中；下载完成、浏览器中断或生成失败都会清理临时目录。同一时刻最多生成两个相簿导出，避免大型相簿把 CPU、磁盘和远端存储连接占满。

“站点设置”恢复原版的公开自定义项：网站名称、标语、作者、头像 URL 和默认浅色/深色/跟随系统主题。它们与存储配置一样保存在 SQLite 中，不需要环境变量；地图、统计脚本、第三方登录等已删除功能不会因此恢复。

## 批量格式转换与中断语义

可勾选一个或多个相簿，将其中的 PNG、JPG/JPEG、WEBP 转为三者之一。任务在 Rust 后台使用固定上限的并发 worker 执行，界面定时读取实时进度；上传和浏览不会被阻塞。

- 每项任务有独立状态（排队、处理中、成功、失败、取消），失败不影响其他图片。
- 取消会停止尚未开始或可撤销的工作；正在进行原子提交的单张图片会安全完成，绝不出现半写入文件。
- 服务重启将未完成任务标为 `interrupted`，保留原图和已经原子提交的转换结果；持久化待提交账本只清理没有数据库记录的孤立对象。
- 转换成功会把新图加入原相簿，旧图默认保留。管理员确认删除后，系统先在同一个数据库事务中写入全部删除授权，再通过持久化 outbox 幂等执行；即使删除途中被强制终止，重启后也会继续完成已确认的删除，而不会误删未确认原图。
- 页面任务中心会在刷新或重新打开后恢复最近 100 个任务及其进度/错误详情；完成前可随时安全中断。

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
- `GET/PATCH /api/albums/:album_id` — 相簿详情及其中的图片，或修改简介和显示日期
- `GET/POST /api/albums/:album_id/photos`
- `GET /api/photos` — 按创建时间倒序列出图片
- `DELETE /api/photos/:photo_id`、`POST /api/photos/delete` — 删除单张或批量删除图片
- `GET /api/photos/:photo_id/file`、`GET /api/photos/:photo_id/thumbnail`
- `GET/POST /api/storage-migrations` — 查看迁移进度或以新的存储配置开始迁移
- `POST /api/storage-migrations/:job_id/resume`、`POST /api/storage-migrations/:job_id/cancel`
- `POST /api/storage-migrations/:job_id/cleanup`、`POST /api/storage-migrations/:job_id/retain` — 删除或保留旧存储图片
- `GET/POST /api/conversions` — 列出任务，或提交 `{ "albumIds": [], "targetFormat": "png|jpg|jpeg|webp" }`
- `GET /api/conversions/:job_id`
- `POST /api/conversions/:job_id/cancel`
- `DELETE /api/conversions/:job_id/delete-sources`

## 验收测试

`scripts/vps-e2e.sh` 会在隔离的 Docker Compose 项目中拉取 `CHRONOFRAME_IMAGE` 指定的 Actions 镜像，覆盖并发首次注册、Argon2id 哈希、Cookie/CSRF、会话过期与退出，以及本地、WebDAV、S3、四种格式互转、多相簿、并行任务、取消、并发读写、硬终止恢复和临时对象清理；`scripts/vps-delete-interrupt.sh` 专门验证登录会话和管理员确认删除后的 outbox 在进程被强制终止时能够安全续作。`scripts/vps-load.py` 用于并发混合负载和延迟阈值检查。

本项目基于原项目的 MIT 许可继续发布，原作者为 HoshinoSuzumi / Timothy Yin。
