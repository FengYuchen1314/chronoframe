FROM node:24-alpine AS web-builder
WORKDIR /src
RUN corepack enable && corepack prepare pnpm@10.34.1 --activate
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml nuxt.config.ts tsconfig.json ./
COPY i18n ./i18n
COPY app ./app
COPY public ./public
COPY shared ./shared
RUN pnpm install --frozen-lockfile
RUN pnpm build

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
RUN useradd --system --create-home chronoframe
WORKDIR /app
COPY --from=api-builder /src/target/release/chronoframe-api /usr/local/bin/chronoframe-api
COPY --from=web-builder /src/.output/public /app/web
RUN mkdir -p /app/data/storage && chown -R chronoframe:chronoframe /app
USER chronoframe
ENV CF_WEB_DIR=/app/web CF_DATABASE_URL=sqlite:///app/data/chronoframe.db?mode=rwc
EXPOSE 8080
CMD ["chronoframe-api"]
