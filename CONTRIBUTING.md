# Contributing

ChronoFrame 由 Nuxt 4 + Vue 3 + TypeScript 前端和 Rust 后端组成。前端生成静态站点并由 Rust 服务统一托管。

## 开发约定

源码可以在本地编辑，但本项目的编译和测试统一在隔离的 VPS 环境执行。不要在本地工作区运行 `pnpm build`、`cargo test` 或 Docker 构建。

VPS 上的前端依赖和构建命令：

```bash
corepack enable
corepack prepare pnpm@10.34.1 --activate
pnpm install --frozen-lockfile
pnpm build
```

VPS 上的 Rust 检查命令：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

涉及存储、上传、转换或中断语义的修改，还要在 VPS 的隔离 Compose 项目中运行：

```bash
bash scripts/vps-e2e.sh
bash scripts/vps-delete-interrupt.sh
```

验收脚本会创建并清理自己的 Docker Compose 测试数据。不要把真实凭据、个人照片或生产数据库放进测试目录。

## 产品约束

- 公共界面沿用原作者的 Nuxt 画廊布局、相簿分类、筛选器和图片查看器；地图与 Globe 功能不再提供。
- 上传入口必须属于已存在的相簿，不能创建无相簿图片。
- 只接受 PNG、JPG/JPEG、WEBP；扩展名、文件签名和完整解码都必须一致。
- 写入必须先进入临时对象，再原子提交；任何新增崩溃窗口都要进入持久化恢复账本。
- 格式转换必须使用受限 worker，不能阻塞上传、读取或 HTTP 运行时。
- 转换完成后默认保留原图；只有管理员明确确认后才可进入持久化删除 outbox。
- 本地磁盘、WebDAV 与 S3 参数只能在后台“存储设置”中管理；密钥不得由读取 API 返回明文，也不得写入日志。

提交信息建议遵循 Conventional Commits。Pull Request 请说明数据安全影响，并附上 VPS 测试结果。
