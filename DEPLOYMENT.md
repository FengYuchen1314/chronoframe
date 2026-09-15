# 部署与运维

Open Gallery 以单个 Docker 镜像发布。部署机器不需要源码、Node.js 或 Rust，只需要 Docker Compose 和一个 `docker-compose.yml`。

每次提交到 `main` 后，GitHub Actions 会在 amd64 与 arm64 原生 runner 上构建并发布多架构镜像 `ghcr.io/uniseem/open-gallery:latest`。

## 快速开始

```bash
mkdir open-gallery && cd open-gallery
curl -fsSLO https://raw.githubusercontent.com/Uniseem/open-gallery/main/docker-compose.yml
docker compose up -d
```

也可以手动新建 `docker-compose.yml`，复制下面的完整内容：

```yaml
# 项目已更名为 open-gallery，但以下名称有意保持 chronoframe，改掉会弄坏已有部署：
# - Compose 项目名与服务名：更新时会另起一套容器，与旧容器争抢同一端口和同一个 ./data；
# - 数据库文件名 chronoframe.db：新容器会找不到旧库，创建一个空数据库。
name: chronoframe

services:
  chronoframe:
    image: ghcr.io/uniseem/open-gallery:latest
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

把文件保存到准备存放相簿数据的目录，在该目录执行 `docker compose up -d`。默认通过 `0.0.0.0:8188` 提供服务。

## 首次注册管理员

全新数据库第一次打开 `/dashboard` 时会显示管理员注册页。第一笔合法注册会在同一个 SQLite 事务中创建管理员和初始会话；一旦创建成功，注册入口永久关闭，之后只能使用该用户名和密码登录。

> **不要让尚未完成首次注册的实例长期暴露在公网**，否则其他访问者可能先行取得管理员身份。

## 访问方式与反向代理

默认同时允许通过公网 `IP:8188`、域名和 HTTP/HTTPS 反向代理访问，管理请求不会因为 Origin、反代协议、域名或端口不同而被拒绝。

程序默认信任 `Forwarded` / `X-Forwarded-Proto`，据此自动决定 Cookie 是否加 `Secure`；代理没有发送这些头时仍可正常使用。

「复制为」图片需要浏览器的安全上下文，因此只在 HTTPS 下可用；通过 HTTP `IP:端口` 访问仍可浏览和下载。

应用本身不限制单次上传的图片数量、单张大小或总大小。若经过第三方反向代理或 CDN，请确认其请求体大小、连接数和超时配置不会额外限制上传。

## 可选环境变量

Compose 无需 `.env`。只有需要修改监听地址、端口或 Cookie 策略时，才参考 `.env.example`：

| 变量 | 默认值 | 说明 |
|---|---|---|
| `CHRONOFRAME_BIND` | `0.0.0.0` | 宿主机监听地址 |
| `CHRONOFRAME_PORT` | `8188` | 宿主机监听端口 |
| `CF_COOKIE_SECURE` | `auto` | `auto`、`true` 或 `false`；`auto` 时反代报告 HTTPS 即使用 Secure Cookie |
| `CF_TRUST_PROXY_HEADERS` | `true` | 是否信任 `Forwarded` 与 `X-Forwarded-Proto` |
| `CF_CONVERSION_WORKERS` | `7` | 仅用于兼容旧版本遗留的转换任务，范围 1–16；新上传的派生图会按 CPU 自动调整并发 |

变量名沿用旧前缀 `CHRONOFRAME_` / `CF_`，以保证已有的 `.env` 继续生效。容器内的 SQLite 路径、主密钥路径、静态前端目录和监听地址已固定在镜像与 Compose 中，无需配置。

## 数据目录

所有持久数据都在 Compose 同目录的 `./data` 中，镜像升级或容器重建不会删除它们：

| 路径 | 内容 |
|---|---|
| `data/chronoframe.db` | SQLite 数据库：相册、图片记录、管理员、存储配置、站点设置、单独上传的相册封面 |
| `data/secret.key` | 加密 WebDAV 密码与 S3 密钥的主密钥，权限固定为 `0600` |
| `data/storage` | 使用本地存储时的原始图片 |
| `data/thumbnails` | 三层派生图缓存，可在后台一键重建 |
| `data/album-downloads` | 公开下载的 ZIP，**始终保存在本地**，不上传 S3/R2 或 WebDAV |

公开下载打包时会预留 256 MiB 磁盘余量；空间不足会停止打包并清理失败文件，释放空间后可在后台重新生成。

## 更新

```bash
docker compose pull
docker compose up -d
```

入口 HTML 始终要求浏览器重新验证，更新后不会因旧页面引用已移除的脚本而白屏。

## 备份与迁移到新机器

迁移前先停止写入，把 Compose 文件和整个 `data` 目录一起打包：

```bash
docker compose stop
sudo tar --numeric-owner -czf open-gallery-backup.tgz docker-compose.yml data
docker compose start
```

主密钥权限为 `0600`，打包必须使用 `sudo` 并确认命令成功。在新机器上用 `sudo tar --numeric-owner -xzf open-gallery-backup.tgz` 解压，进入目录执行 `docker compose up -d` 即可。

**数据库与主密钥必须一起备份**，缺少任意一项都无法恢复存储凭据。使用 WebDAV 或 S3 时，目录备份只包含数据库、主密钥和连接配置，远端图片仍在原存储中；需要离线完整迁移时还要另行迁移远端对象。

## 存储后端

存储连接参数全部在后台「存储与维护」中配置并保存在数据库里，程序不会从环境变量读取本地路径、WebDAV 或 S3 参数。保存前可先执行写入、读回、删除的连接测试。

- **本地存储**：保持默认路径 `./data/storage`。填写 `/app/data` 之外的容器路径不会被 Compose 持久化，也不会进入上面的目录备份；在本地存储之间迁移时，目标路径同样须位于 `/app/data` 下，例如 `/app/data/storage-new`。
- **WebDAV**：服务端需支持 `MKCOL`、`PUT`、`MOVE`、`DELETE`。
- **S3 / R2**：需兼容 path-style 请求。凭据除读写对象外，使用「S3 旧空间回收」还需要 `ListBucket` 与删除对象权限。Cloudflare R2 的区域填 `auto`。

没有图片时可直接切换存储；已有图片时，修改类型、路径、Endpoint、桶或前缀会进入安全迁移流程。只轮换同一目标的凭据不会触发迁移。

## 从旧版本升级

- **旧 Compose 使用 `build: .`**：`docker compose pull` 不会更新应用。请改用上面示例中的 `image:`，保留原来的 `volumes` 映射，再执行更新命令。
- **从 `X-Admin-Token` 版本升级**：第一次启动新版时可暂时保留原 `CF_ADMIN_TOKEN`。仅当主密钥文件尚不存在时，程序会一次性用旧令牌派生兼容密钥并写入 `CF_MASTER_KEY_FILE`；确认密钥文件生成后即可移除该变量，它不再用于登录或鉴权。
- **公开下载**：从不支持公开下载的版本升级后，所有相册默认关闭下载，需按相册开启。升级前已失败的打包任务需点击「重新生成」才会使用新的重试策略。

## 镜像地址变更

本项目曾以 ChronoFrame 为名发布，镜像地址变化如下：

| 地址 | 状态 |
|---|---|
| `ghcr.io/uniseem/open-gallery` | **当前地址**，持续更新 |
| `ghcr.io/uniseem/chronoframe` | 归档保留，不再更新 |
| `ghcr.io/fengyuchen1314/chronoframe` | 归档保留，不再更新 |

归档地址中的历史版本仍可拉取。按旧地址部署的实例，只需把 `docker-compose.yml` 里的 `image:` 改为 `ghcr.io/uniseem/open-gallery:latest`，再执行上面的更新命令；Compose 项目名、服务名、数据库文件名与环境变量均未改变，数据不受影响。

仓库原位于 `FengYuchen1314/chronoframe` 与 `Uniseem/chronoframe`，旧地址由 GitHub 自动重定向。
