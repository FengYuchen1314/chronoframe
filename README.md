# ChronoFrame

面向自托管的相簿优先画廊。项目已从原 Nuxt/Node 实现迁移为：

- `frontend/`：React + TypeScript + Vite 单页界面
- `backend/`：Rust + Axum + SQLite API
- 存储：本地磁盘、WebDAV 或 S3 兼容对象存储

地图、地理编码、EXIF 地图探索和“所有图片”入口均不再存在。根页面默认且只以相簿空间作为浏览入口：先创建相簿，之后才能上传图片。

## 本地开发

```powershell
Copy-Item .env.example .env
# 编辑 .env，将 CF_ADMIN_TOKEN 改为高强度值
cd frontend; npm install; npm run dev
# 新开终端，但保持在项目根目录
cargo run --manifest-path backend/Cargo.toml
```

开发前端位于 `http://localhost:5173`，并代理 API 到 Rust 服务的 `http://localhost:8080`。生产构建：

```powershell
cd frontend; npm run build
cd ..; cargo run --release --manifest-path backend/Cargo.toml
```

也可运行 `docker compose up --build`。容器服务监听 `8080`。

## 存储后端

所有存储连接参数都在管理员界面的“存储设置”中维护，并持久化到 SQLite；程序不会从 `.env` 读取本地路径、WebDAV 或 S3 参数。后台可先执行完整的写入、读回、删除连接测试，再保存设置。已有图片或待清理对象时禁止直接切换存储目标，避免数据库记录指向另一套存储；同一目标的凭据轮换仍然允许。

环境变量只负责服务运行时，不负责存储：

- `CF_ADMIN_TOKEN`：管理员令牌，同时用于派生存储密钥加密密钥。
- `CF_DATABASE_URL`：SQLite 数据库位置。
- `CF_CONVERSION_WORKERS`：全局转换 worker 上限，限制为 1–16。
- `CF_WEB_DIR`、`CF_BIND_ADDR`：前端静态目录和监听地址。

WebDAV 密码和 S3 秘密访问密钥使用从管理员令牌派生的 AES-256-GCM 密钥加密后存储，读取设置时永不返回明文；若更换 `CF_ADMIN_TOKEN`，需要在后台重新保存相应密钥。上传和转换在 WebDAV 中会先写入临时对象，再使用 `MOVE` 原子提交；S3 使用临时对象复制到最终键；本地存储则先写入同目录临时文件后重命名。请使用支持 WebDAV `MKCOL`、`PUT`、`MOVE`、`DELETE` 的服务端，以及兼容 S3 path-style 请求的对象存储服务。

上传必须指定一个已存在的相簿。单次请求最多 128 张、总计 384 MiB、单张 100 MiB；服务会在写入任何对象前校验整批文件的扩展名、文件签名和完整解码，批次中任一文件无效时不会留下部分上传。

## 批量格式转换与中断语义

可勾选一个或多个相簿，将其中的 PNG、JPG/JPEG、WEBP 转为三者之一。任务在 Rust 后台使用固定上限的并发 worker 执行，界面定时读取实时进度；上传和浏览不会被阻塞。

- 每项任务有独立状态（排队、处理中、成功、失败、取消），失败不影响其他图片。
- 取消会停止尚未开始或可撤销的工作；正在进行原子提交的单张图片会安全完成，绝不出现半写入文件。
- 服务重启将未完成任务标为 `interrupted`，保留原图和已经原子提交的转换结果；持久化待提交账本只清理没有数据库记录的孤立对象。
- 转换成功会把新图加入原相簿，旧图默认保留。管理员确认删除后，系统先在同一个数据库事务中写入全部删除授权，再通过持久化 outbox 幂等执行；即使删除途中被强制终止，重启后也会继续完成已确认的删除，而不会误删未确认原图。
- 页面任务中心会在刷新或重新打开后恢复最近 100 个任务及其进度/错误详情；完成前可随时安全中断。

所有写操作都要求 `X-Admin-Token`，React 界面仅将它保存于当前浏览器会话中。

## API 摘要

- `GET/PUT /api/settings/storage` — 管理员读取或保存存储后端设置
- `POST /api/settings/storage/test` — 在不保存的情况下测试候选存储
- `GET/POST /api/albums`
- `GET/POST /api/albums/:album_id/photos`
- `GET /api/photos/:photo_id/file`
- `GET/POST /api/conversions` — 列出任务，或提交 `{ "albumIds": [], "targetFormat": "png|jpg|jpeg|webp" }`
- `GET /api/conversions/:job_id`
- `POST /api/conversions/:job_id/cancel`
- `DELETE /api/conversions/:job_id/delete-sources`

## 验收测试

`scripts/vps-e2e.sh` 会在隔离的 Docker Compose 项目中覆盖本地、WebDAV、S3、四种格式互转、多相簿、并行任务、取消、并发读写、硬终止恢复和临时对象清理；`scripts/vps-delete-interrupt.sh` 专门验证管理员确认删除后的 outbox 在进程被强制终止时能够安全续作。`scripts/vps-load.py` 用于并发混合负载和延迟阈值检查。

本项目基于原项目的 MIT 许可继续发布，原作者为 HoshinoSuzumi / Timothy Yin。
