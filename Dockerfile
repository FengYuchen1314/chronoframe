# 管理后台是独立构建的 React SPA（admin/，自带 lockfile 与工具链）。
# 单独一个阶段构建，产物在下一步被放进 public/dashboard，
# 随 Nuxt 的静态产物一起发布。两个工具链互不干扰。
FROM node:24-alpine AS admin-builder
WORKDIR /admin
RUN corepack enable && corepack prepare pnpm@10.34.1 --activate
COPY admin/package.json admin/pnpm-lock.yaml admin/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY admin ./
RUN pnpm build

FROM node:24-alpine AS web-builder
WORKDIR /src
RUN corepack enable && corepack prepare pnpm@10.34.1 --activate
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml nuxt.config.ts tsconfig.json ./
COPY i18n ./i18n
COPY app ./app
COPY public ./public
COPY shared ./shared
RUN pnpm install --frozen-lockfile
# nuxt generate 会把 public/ 原样复制到 .output/public，管理后台由此进入静态产物。
COPY --from=admin-builder /admin/dist ./public/dashboard
# 必须是 build:web（纯 nuxt generate），不能用 pnpm build：后者还会跑
# scripts/build-admin.mjs 重新构建管理后台，而本阶段既没有 admin/ 也没有
# scripts/（scripts 已被 .dockerignore 排除），会直接 MODULE_NOT_FOUND。
# 后台产物已由上面的 admin-builder 阶段交付。
RUN pnpm build:web

FROM rust:1.96-bookworm AS api-builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY backend/Cargo.toml backend/Cargo.toml
RUN mkdir backend/src && printf 'fn main() {}' > backend/src/main.rs && cargo build --release -p chronoframe-api && rm -rf backend/src
COPY backend backend
# The dependency warm-up stage compiles a placeholder main. Touch the real entrypoint so
# Docker's normalized COPY timestamps can never reuse that placeholder executable.
RUN touch backend/src/main.rs && cargo build --release -p chronoframe-api

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates gosu \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 chronoframe \
    && useradd --uid 10001 --gid chronoframe --create-home --shell /usr/sbin/nologin chronoframe
WORKDIR /app
COPY --from=api-builder /src/target/release/chronoframe-api /usr/local/bin/chronoframe-api
COPY --from=web-builder /src/.output/public /app/web
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod 0755 /usr/local/bin/docker-entrypoint.sh \
    && mkdir -p /app/data/storage \
    && chown -R chronoframe:chronoframe /app
ENV CF_WEB_DIR=/app/web CF_DATABASE_URL=sqlite:///app/data/chronoframe.db?mode=rwc
LABEL org.opencontainers.image.source="https://github.com/FengYuchen1314/chronoframe" \
      org.opencontainers.image.description="Album-first self-hosted gallery with a Rust backend" \
      org.opencontainers.image.licenses="MIT"
EXPOSE 8080
ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["chronoframe-api"]
