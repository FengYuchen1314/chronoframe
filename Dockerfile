FROM node:24-alpine AS web-builder
WORKDIR /src/frontend
COPY frontend/package*.json ./
RUN npm install
COPY frontend ./
RUN npm run build

FROM rust:1.96-bookworm AS api-builder
WORKDIR /src
COPY Cargo.toml ./
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
COPY --from=web-builder /src/frontend/dist /app/web
RUN mkdir -p /app/data/storage && chown -R chronoframe:chronoframe /app
USER chronoframe
ENV CF_WEB_DIR=/app/web CF_DATABASE_URL=sqlite:///app/data/chronoframe.db?mode=rwc
EXPOSE 8080
CMD ["chronoframe-api"]
