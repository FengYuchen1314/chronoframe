# Contributing

ChronoFrame 现在由 React + TypeScript 前端和 Rust 后端组成。提交修改前请安装 Node.js 20+、Rust 1.85+，可选安装 Docker。

## 开发

```powershell
Copy-Item .env.example .env
cd frontend
npm install
npm run dev
```

另开一个终端，在仓库根目录运行：

```powershell
cargo run --manifest-path backend/Cargo.toml
```

Vite 开发服务监听 `http://localhost:5173` 并代理到 Rust API 的 `http://localhost:8080`。

存储参数不得加入环境变量或源码。请用管理员令牌进入页面，在“存储设置”中配置本地磁盘、WebDAV 或 S3，并通过连接测试后保存。环境变量只用于管理员令牌、SQLite 地址、worker 数量、静态文件目录和监听地址。

## 提交前检查

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cd frontend
npm run build
```

涉及存储、上传、转换或中断语义的修改，还应在隔离环境运行：

```bash
bash scripts/vps-e2e.sh
bash scripts/vps-delete-interrupt.sh
```

验收脚本会创建并清理自己的 Docker Compose 测试数据。不要把真实凭据、个人照片或生产数据库放进测试目录。

## 约束

- 上传入口必须属于已存在的相簿，首页保持相簿优先。
- 只接受 PNG、JPG/JPEG、WEBP；扩展名、文件签名和完整解码都必须一致。
- 写入必须先进入临时对象，再原子提交；任何新增崩溃窗口都要进入持久化恢复账本。
- 格式转换必须使用受限 worker，不能阻塞上传、读取或 HTTP 运行时。
- 转换完成后默认保留原图；只有管理员明确确认后才可进入持久化删除 outbox。
- 存储密钥不得由读取 API 返回明文，也不得写入日志。

提交信息建议遵循 Conventional Commits。Pull Request 请说明数据安全影响，并附上对应测试结果。
