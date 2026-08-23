# ChronoFrame

面向自托管的个人画廊。项目恢复原作者的 Nuxt 视觉框架，并将服务端重构为：

- 根目录 `app/`、`i18n/`、`shared/`、`public/`：Nuxt 4 + Vue 3 + TypeScript 静态前端
- `backend/`：Rust + Axum + SQLite API
- 存储：本地磁盘、WebDAV 或 S3 兼容对象存储

公共端恢复原版的相簿动效主页、照片瀑布流、标签/相机/镜头/城市/评分筛选、排序、相簿详情和沉浸式查看器；地图、Globe 和地图管理功能不再存在。默认 `/` 直接展示相簿空间，全图瀑布流位于 `/photos`。上传严格遵循相簿优先的数据规则：管理员必须先创建相簿，之后才能向其中上传图片。

## 单文件部署

提交到 `main` 后，GitHub Actions 会在原生 amd64 和 arm64 runner 上编译 Nuxt/Rust、构建镜像并发布多架构的 `ghcr.io/fengyuchen1314/chronoframe:latest`。部署机器不需要源码、Node.js 或 Rust，只需要 Docker Compose 和根目录的一个 `docker-compose.yml`。

```bash
mkdir chronoframe && cd chronoframe
curl -fsSLO https://raw.githubusercontent.com/FengYuchen1314/chronoframe/main/docker-compose.yml
docker compose up -d
```

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

Compose 无需 `.env`。如需覆盖端口或 HTTPS 策略，可在同目录创建可选 `.env`，可用变量见 `.env.example`。直连 HTTP 使用默认的 `CF_COOKIE_SECURE=false`。配置 HTTPS 反向代理时，应将 `CHRONOFRAME_BIND=127.0.0.1`、`CF_COOKIE_SECURE=true`；只有代理会覆盖客户端传入的转发头时才设置 `CF_TRUST_PROXY_HEADERS=true`。

全新数据库第一次打开 `/dashboard` 时会显示管理员注册页。第一笔合法注册会在同一个 SQLite 事务中创建管理员和初始会话；一旦创建成功，注册入口永久关闭，之后只能使用该用户名和密码登录。不要让尚未完成首次注册的实例长期暴露在公网，否则其他访问者可能先行取得管理员身份。生产环境应通过 HTTPS 反向代理访问后台，并将 `CF_COOKIE_SECURE=true`；若需要使用代理提供的外部协议或主机头，还须在后端端口不对公网开放且代理会覆盖客户端同名请求头的前提下设置 `CF_TRUST_PROXY_HEADERS=true`。直接 HTTP 只适合隔离测试或通过 SSH 隧道访问。

## 存储后端

所有存储连接参数都在管理员界面的“存储设置”中维护，并持久化到 SQLite；程序不会从 `.env` 读取本地路径、WebDAV 或 S3 参数。后台可先执行完整的写入、读回、删除连接测试，再保存设置。已有图片或待清理对象时禁止直接切换存储目标，避免数据库记录指向另一套存储；同一目标的凭据轮换仍然允许。

单文件 Docker 部署使用本地存储时，请保持后台默认路径 `./data/storage`；它对应宿主机当前目录的 `./data/storage`。改到 `/app/data` 之外的容器路径不会被 Compose 持久化，也不会进入上述目录备份。

Compose 变量只负责服务运行时，不负责存储：

- `CHRONOFRAME_BIND`、`CHRONOFRAME_PORT`：宿主机监听地址和端口。
- `CF_COOKIE_SECURE`：`auto`、`true` 或 `false`；HTTPS 生产环境应为 `true`。
- `CF_TRUST_PROXY_HEADERS`：是否信任 `Forwarded`、`X-Forwarded-Proto` 和 `X-Forwarded-Host`；默认 `false`，只可对受控反向代理开启。
- `CF_CONVERSION_WORKERS`：全局转换 worker 上限，限制为 1–16。

容器内部的 SQLite 路径、主密钥路径、静态前端目录和监听地址已固定在镜像与 Compose 中，无需用户配置。

WebDAV 密码和 S3 秘密访问密钥使用独立安装主密钥进行 AES-256-GCM 加密，读取设置时永不返回明文。主密钥与管理员密码完全解耦；备份或迁移时必须同时保存 SQLite 数据库和 `CF_MASTER_KEY_FILE`，缺少任意一项都无法恢复存储凭据。上传和转换在 WebDAV 中会先写入临时对象，再使用 `MOVE` 原子提交；S3 使用临时对象复制到最终键；本地存储则先写入同目录临时文件后重命名。请使用支持 WebDAV `MKCOL`、`PUT`、`MOVE`、`DELETE` 的服务端，以及兼容 S3 path-style 请求的对象存储服务。

从旧的 `X-Admin-Token` 版本升级时，可在第一次启动新版时暂时保留原 `CF_ADMIN_TOKEN`。只有在主密钥文件尚不存在时，程序才会一次性用旧令牌派生兼容密钥并写入 `CF_MASTER_KEY_FILE`；确认密钥文件已经生成后即可移除该环境变量。它不会再被用于登录或 API 鉴权。

上传必须指定一个已存在的相簿。单次请求最多 128 张、总计 384 MiB、单张 100 MiB；服务会在写入任何对象前校验整批文件的扩展名、文件签名和完整解码，批次中任一文件无效时不会留下部分上传。

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
- `GET/POST /api/albums`
- `GET /api/albums/:album_id` — 相簿详情及其中的图片
- `GET/POST /api/albums/:album_id/photos`
- `GET /api/photos` — 按创建时间倒序列出图片
- `GET /api/photos/:photo_id/file`、`GET /api/photos/:photo_id/thumbnail`
- `GET/POST /api/conversions` — 列出任务，或提交 `{ "albumIds": [], "targetFormat": "png|jpg|jpeg|webp" }`
- `GET /api/conversions/:job_id`
- `POST /api/conversions/:job_id/cancel`
- `DELETE /api/conversions/:job_id/delete-sources`

## 验收测试

`scripts/vps-e2e.sh` 会在隔离的 Docker Compose 项目中拉取 `CHRONOFRAME_IMAGE` 指定的 Actions 镜像，覆盖并发首次注册、Argon2id 哈希、Cookie/CSRF、会话过期与退出，以及本地、WebDAV、S3、四种格式互转、多相簿、并行任务、取消、并发读写、硬终止恢复和临时对象清理；`scripts/vps-delete-interrupt.sh` 专门验证登录会话和管理员确认删除后的 outbox 在进程被强制终止时能够安全续作。`scripts/vps-load.py` 用于并发混合负载和延迟阈值检查。

本项目基于原项目的 MIT 许可继续发布，原作者为 HoshinoSuzumi / Timothy Yin。
