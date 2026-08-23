# ChronoFrame development notes

ChronoFrame is an album-first self-hosted gallery. The browser client is React + TypeScript in `frontend/`; the API and background workers are Rust/Axum in `backend/`.

- Keep the root view album-first. An album must exist before uploads are accepted.
- Supported image formats are PNG, JPG/JPEG and WEBP only.
- Do not add map, geocoding, EXIF exploration or an all-photos entry point.
- Storage connections are configured only through the authenticated admin UI and SQLite settings. Never introduce WebDAV/S3/local-storage environment variables.
- Treat every blob write as a crash-consistency boundary. Use atomic staging and the durable pending ledger.
- Conversions use bounded global workers, report persistent progress, support cancellation, and preserve source images until explicit admin confirmation.
- Confirmed source deletion must remain resumable through the durable deletion outbox.
- Never return storage secrets from read APIs or log them.

Before submitting:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cd frontend && npm run build
```

Storage or interruption changes also require the isolated VPS E2E and deletion-interruption suites in `scripts/`.
