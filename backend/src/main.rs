use std::{
    collections::{HashMap, HashSet},
    env,
    io::{Cursor, ErrorKind, Write},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, AeadCore, OsRng as AeadOsRng},
};
use anyhow::{Context, Result, anyhow, bail};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_sdk_s3::{Client as S3Client, config::Builder as S3ConfigBuilder};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{
        DefaultBodyLimit, Multipart, Path as AxumPath, Query, State,
        rejection::{BytesRejection, JsonRejection},
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use dashmap::DashMap;
use futures_util::{
    Stream,
    stream::{self, StreamExt},
};
use image::ImageFormat;
use rand::{RngCore, rngs::OsRng};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, sqlite::SqlitePoolOptions};
use subtle::ConstantTimeEq;
use tokio::{io::AsyncWriteExt, time::timeout};
use tokio_util::{io::ReaderStream, sync::CancellationToken};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::{error, info, warn};
use uuid::Uuid;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

mod album_covers;
mod album_downloads;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    storage: StorageService,
    secure_cookies: Option<bool>,
    trust_proxy_headers: bool,
    workers: usize,
    jobs: Arc<DashMap<String, CancellationToken>>,
    storage_tasks: Arc<DashMap<String, CancellationToken>>,
    thumbnail_tasks: Arc<DashMap<String, CancellationToken>>,
    s3_cleanup_tasks: Arc<DashMap<String, CancellationToken>>,
    storage_mutation_gate: Arc<tokio::sync::RwLock<()>>,
    conversion_slots: Arc<tokio::sync::Semaphore>,
    export_slots: Arc<tokio::sync::Semaphore>,
    upload_slots: Arc<tokio::sync::Semaphore>,
    thumbnail_slots: Arc<tokio::sync::Semaphore>,
    thumbnail_workers: usize,
    password_hash_slots: Arc<tokio::sync::Semaphore>,
    photo_graph_lock: Arc<tokio::sync::Mutex<()>>,
    downloads: Arc<album_downloads::Service>,
}

const STORAGE_IO_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_UPLOAD_CONCURRENCY: usize = 7;
const DEFAULT_CONVERSION_CONCURRENCY: usize = 7;
const DEFAULT_SOURCE_DELETE_CONCURRENCY: usize = 7;
const DEFAULT_THUMBNAIL_CONCURRENCY: usize = 7;
const DEFAULT_S3_CLEANUP_CONCURRENCY: usize = 8;
const S3_ORPHAN_GRACE_SECONDS: i64 = 24 * 60 * 60;
const GRID_THUMBNAIL_LONGEST_EDGE: u32 = 320;
const VIEW_PREVIEW_LONGEST_EDGE: u32 = 2560;
const VIEW_PREVIEW_MAX_BYTES: usize = 1_500_000;
const VIEW_HIGH_LONGEST_EDGE: u32 = 4096;
const VIEW_HIGH_MAX_BYTES: usize = 5_000_000;
const SESSION_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;
const SESSION_COOKIE: &str = "cf_session";
const CSRF_COOKIE: &str = "cf_csrf";
const REQUESTED_WITH: &str = "ChronoFrame";

fn staging_key(key: &str) -> String {
    format!("{key}.cf-pending")
}

fn validate_storage_prefix(prefix: &str) -> Result<()> {
    if prefix.len() > 512 {
        bail!("存储目录前缀过长");
    }
    if prefix.is_empty() {
        return Ok(());
    }
    for segment in prefix.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || !segment.chars().all(|character| {
                character.is_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
        {
            bail!("存储目录前缀只能包含字母、数字、点、短横线、下划线和安全的斜杠分段");
        }
    }
    Ok(())
}

#[derive(Clone)]
struct Config {
    database_url: String,
    master_key_file: PathBuf,
    thumbnail_cache_dir: PathBuf,
    secure_cookies: Option<bool>,
    trust_proxy_headers: bool,
    workers: usize,
    web_dir: String,
}

impl Config {
    fn from_env() -> Result<Self> {
        let get =
            |name: &str, default: &str| env::var(name).unwrap_or_else(|_| default.to_string());
        let database_url = get("CF_DATABASE_URL", "sqlite://data/chronoframe.db?mode=rwc");
        let default_master_key = database_path(&database_url)
            .and_then(|path| path.parent().map(|parent| parent.join("secret.key")))
            .unwrap_or_else(|| PathBuf::from("data/secret.key"));
        let thumbnail_cache_dir = database_path(&database_url)
            .and_then(|path| path.parent().map(|parent| parent.join("thumbnails")))
            .unwrap_or_else(|| PathBuf::from("data/thumbnails"));
        Ok(Self {
            database_url,
            master_key_file: env::var("CF_MASTER_KEY_FILE")
                .map(PathBuf::from)
                .unwrap_or(default_master_key),
            thumbnail_cache_dir,
            secure_cookies: match get("CF_COOKIE_SECURE", "auto")
                .trim()
                .to_ascii_lowercase()
                .as_str()
            {
                "auto" => None,
                "true" | "1" | "yes" => Some(true),
                "false" | "0" | "no" => Some(false),
                _ => bail!("CF_COOKIE_SECURE must be auto, true, or false"),
            },
            trust_proxy_headers: match get("CF_TRUST_PROXY_HEADERS", "true")
                .trim()
                .to_ascii_lowercase()
                .as_str()
            {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => bail!("CF_TRUST_PROXY_HEADERS must be true or false"),
            },
            workers: get("CF_CONVERSION_WORKERS", "7")
                .parse::<usize>()
                .unwrap_or(DEFAULT_CONVERSION_CONCURRENCY)
                .clamp(1, 16),
            web_dir: get("CF_WEB_DIR", "./.output/public"),
        })
    }
}

fn database_path(database_url: &str) -> Option<PathBuf> {
    let value = database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))?
        .split('?')
        .next()?;
    if value.is_empty() || value == ":memory:" {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

async fn restrict_secret_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn legacy_storage_key(seed: &str) -> [u8; 32] {
    let digest = Sha256::digest(seed.as_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

async fn read_master_key(path: &Path) -> Result<[u8; 32]> {
    for attempt in 0..20 {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("无法读取主密钥 {}", path.display()))?;
        if bytes.len() == 32 {
            restrict_secret_permissions(path).await?;
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
        if attempt < 19 {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    bail!("主密钥文件 {} 必须恰好为 32 字节", path.display())
}

async fn load_or_create_master_key(path: &Path) -> Result<[u8; 32]> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("无法创建主密钥目录 {}", parent.display()))?;
    }

    match tokio::fs::metadata(path).await {
        Ok(_) => return read_master_key(path).await,
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("无法检查主密钥 {}", path.display()));
        }
    }

    // CF_ADMIN_TOKEN is consulted only while bootstrapping a missing key file, solely to preserve
    // decryptability of credentials encrypted by pre-account releases. It is never authentication.
    let legacy_admin_token = env::var("CF_ADMIN_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    let generated = legacy_admin_token
        .as_deref()
        .map(legacy_storage_key)
        .unwrap_or_else(|| {
            let mut key = [0u8; 32];
            OsRng.fill_bytes(&mut key);
            key
        });
    let create_path = path.to_owned();
    let create_key = generated;
    let created = tokio::task::spawn_blocking(move || {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(create_path)?;
        std::io::Write::write_all(&mut file, &create_key)?;
        file.sync_all()
    })
    .await
    .context("主密钥创建任务异常退出")?;
    match created {
        Ok(()) => {
            restrict_secret_permissions(path).await?;
            if legacy_admin_token.is_some() {
                info!(path = %path.display(), "migrated legacy storage encryption key to master key file");
            } else {
                info!(path = %path.display(), "created storage master key");
            }
            Ok(generated)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            // Another process won create_new. Retry a briefly partial file until its sync completes.
            read_master_key(path).await
        }
        Err(error) => Err(error).with_context(|| format!("无法创建主密钥 {}", path.display())),
    }
}

#[async_trait]
trait BlobStore: Send + Sync {
    async fn put_atomic(&self, key: &str, content_type: &str, data: Vec<u8>) -> Result<()>;
    async fn get(&self, key: &str) -> Result<Vec<u8>>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn healthcheck(&self) -> Result<()> {
        // A fixed probe key bounds any artifact left by a process kill during a connection test to one object per configured target.
        let key = "system/healthcheck.bin".to_string();
        let _ = self.delete(&staging_key(&key)).await;
        let _ = self.delete(&key).await;
        let expected = b"chronoframe-storage-healthcheck".to_vec();
        if let Err(error) = self
            .put_atomic(&key, "application/octet-stream", expected.clone())
            .await
        {
            let _ = self.delete(&key).await;
            let _ = self.delete(&staging_key(&key)).await;
            return Err(error);
        }
        let check = async {
            let actual = self.get(&key).await?;
            if actual != expected {
                bail!("存储读回的数据与写入内容不一致");
            }
            Ok(())
        }
        .await;
        let cleanup = self.delete(&key).await;
        check?;
        cleanup.context("存储可读写，但无法删除测试对象")
    }
}

struct LocalStore {
    root: PathBuf,
}

impl LocalStore {
    fn path(&self, key: &str) -> Result<PathBuf> {
        if key
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.contains('\\'))
        {
            bail!("unsafe object key");
        }
        Ok(self.root.join(key))
    }
}

#[async_trait]
impl BlobStore for LocalStore {
    async fn put_atomic(&self, key: &str, _: &str, data: Vec<u8>) -> Result<()> {
        let target = self.path(key)?;
        let parent = target
            .parent()
            .ok_or_else(|| anyhow!("object key has no parent"))?
            .to_owned();
        tokio::fs::create_dir_all(&parent).await?;
        let temp = self.path(&staging_key(key))?;
        let result = async {
            let mut file = tokio::fs::File::create(&temp).await?;
            file.write_all(&data).await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&temp, &target).await?;
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temp).await;
        }
        result
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        Ok(tokio::fs::read(self.path(key)?).await?)
    }
    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.path(key)?;
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

struct WebDavStore {
    client: Client,
    base_url: url::Url,
    authorization: HeaderValue,
    prefix: String,
}

impl WebDavStore {
    fn key(&self, key: &str) -> Result<String> {
        if key
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.contains('\\'))
        {
            bail!("unsafe object key");
        }
        Ok(if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}/{key}", self.prefix)
        })
    }
    fn url(&self, key: &str) -> Result<url::Url> {
        Ok(self.base_url.join(&self.key(key)?)?)
    }
    async fn make_parents(&self, key: &str) -> Result<()> {
        let full = self.key(key)?;
        let parts: Vec<&str> = full.split('/').collect();
        for end in 1..parts.len() {
            let path = format!("{}/", parts[..end].join("/"));
            let response = self
                .client
                .request(Method::from_bytes(b"MKCOL")?, self.base_url.join(&path)?)
                .header(header::AUTHORIZATION, self.authorization.clone())
                .send()
                .await?;
            if !(response.status().is_success()
                || response.status() == StatusCode::METHOD_NOT_ALLOWED)
            {
                bail!("WebDAV MKCOL failed: {}", response.status());
            }
        }
        Ok(())
    }
}

#[async_trait]
impl BlobStore for WebDavStore {
    async fn put_atomic(&self, key: &str, content_type: &str, data: Vec<u8>) -> Result<()> {
        self.make_parents(key).await?;
        let temp = staging_key(key);
        let put = self
            .client
            .put(self.url(&temp)?)
            .header(header::AUTHORIZATION, self.authorization.clone())
            .header(header::CONTENT_TYPE, content_type)
            .body(data)
            .send()
            .await;
        let put = match put {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                let _ = self.delete(&temp).await;
                bail!("WebDAV temporary upload failed: {}", response.status());
            }
            Err(error) => {
                let _ = self.delete(&temp).await;
                return Err(error).context("WebDAV temporary upload failed");
            }
        };
        drop(put);
        let destination = self.url(key)?.to_string();
        let moved = self
            .client
            .request(Method::from_bytes(b"MOVE")?, self.url(&temp)?)
            .header(header::AUTHORIZATION, self.authorization.clone())
            .header("Destination", destination)
            .header("Overwrite", "F")
            .send()
            .await;
        match moved {
            Ok(response) if response.status().is_success() => Ok(()),
            Ok(response) => {
                let _ = self.delete(&temp).await;
                bail!("WebDAV atomic move failed: {}", response.status());
            }
            Err(error) => {
                let _ = self.delete(&temp).await;
                Err(error).context("WebDAV atomic move failed")
            }
        }
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(self.url(key)?)
            .header(header::AUTHORIZATION, self.authorization.clone())
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("WebDAV read failed: {}", response.status());
        }
        Ok(response.bytes().await?.to_vec())
    }
    async fn delete(&self, key: &str) -> Result<()> {
        let response = self
            .client
            .delete(self.url(key)?)
            .header(header::AUTHORIZATION, self.authorization.clone())
            .send()
            .await?;
        if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            bail!("WebDAV delete failed: {}", response.status())
        }
    }
}

struct S3Store {
    client: S3Client,
    bucket: String,
    prefix: String,
}

#[derive(Clone, Debug)]
struct S3ManagedObject {
    physical_key: String,
    logical_key: String,
    byte_size: i64,
    last_modified: i64,
}

impl S3Store {
    fn key(&self, key: &str) -> Result<String> {
        if key
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.contains('\\'))
        {
            bail!("unsafe object key");
        }
        Ok(if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}/{key}", self.prefix)
        })
    }

    fn managed_prefix(&self) -> String {
        if self.prefix.is_empty() {
            "albums/".to_string()
        } else {
            format!("{}/albums/", self.prefix)
        }
    }

    fn logical_key(&self, physical_key: &str) -> Option<String> {
        let prefix = if self.prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", self.prefix)
        };
        physical_key
            .strip_prefix(&prefix)
            .filter(|key| key.starts_with("albums/"))
            .map(str::to_string)
    }

    async fn list_managed_objects_page(
        &self,
        continuation_token: Option<&str>,
    ) -> Result<(Vec<S3ManagedObject>, Option<String>)> {
        let mut request = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(self.managed_prefix());
        if let Some(token) = continuation_token {
            request = request.continuation_token(token);
        }
        let page = timeout(STORAGE_IO_TIMEOUT, request.send())
            .await
            .context("S3 对象列表请求超时")??;
        let mut objects = Vec::with_capacity(page.contents().len());
        for object in page.contents() {
            let Some(physical_key) = object.key() else {
                continue;
            };
            let Some(logical_key) = self.logical_key(physical_key) else {
                continue;
            };
            objects.push(S3ManagedObject {
                physical_key: physical_key.to_string(),
                logical_key,
                byte_size: object.size().unwrap_or_default().max(0),
                // Missing timestamps are treated as new, so an unusual S3 implementation can
                // never make an unknown object eligible for deletion.
                last_modified: object
                    .last_modified()
                    .map(|value| value.secs())
                    .unwrap_or(i64::MAX),
            });
        }
        let next = if page.is_truncated() == Some(true) {
            Some(
                page.next_continuation_token()
                    .context("S3 返回了截断列表但没有下一页标记")?
                    .to_string(),
            )
        } else {
            None
        };
        Ok((objects, next))
    }

    async fn delete_physical_object(&self, physical_key: &str) -> Result<()> {
        if self.logical_key(physical_key).is_none() {
            bail!("拒绝删除 ChronoFrame 管理前缀之外的 S3 对象");
        }
        timeout(
            STORAGE_IO_TIMEOUT,
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(physical_key)
                .send(),
        )
        .await
        .context("S3 旧对象删除超时")??;
        Ok(())
    }
}

#[async_trait]
impl BlobStore for S3Store {
    async fn put_atomic(&self, key: &str, content_type: &str, data: Vec<u8>) -> Result<()> {
        let temp = self.key(&staging_key(key))?;
        let key = self.key(key)?;
        timeout(
            STORAGE_IO_TIMEOUT,
            self.client
                .put_object()
                .bucket(&self.bucket)
                .key(&temp)
                .content_type(content_type)
                .body(data.into())
                .send(),
        )
        .await
        .context("S3 temporary upload timed out")??;
        // S3 makes a copied destination visible atomically. The temporary object is never referenced by the gallery database.
        if let Err(error) = timeout(
            STORAGE_IO_TIMEOUT,
            self.client
                .copy_object()
                .bucket(&self.bucket)
                .key(&key)
                .copy_source(format!("{}/{}", self.bucket, urlencoding::encode(&temp)))
                .send(),
        )
        .await
        .context("S3 atomic copy timed out")
        .and_then(|result| result.map_err(Into::into))
        {
            let _ = timeout(
                STORAGE_IO_TIMEOUT,
                self.client
                    .delete_object()
                    .bucket(&self.bucket)
                    .key(&temp)
                    .send(),
            )
            .await;
            return Err(error);
        }
        if let Err(error) = timeout(
            STORAGE_IO_TIMEOUT,
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(&temp)
                .send(),
        )
        .await
        .context("S3 staging cleanup timed out")
        .and_then(|result| result.map(|_| ()).map_err(Into::into))
        {
            let _ = timeout(
                STORAGE_IO_TIMEOUT,
                self.client
                    .delete_object()
                    .bucket(&self.bucket)
                    .key(&key)
                    .send(),
            )
            .await;
            return Err(error);
        }
        Ok(())
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let result = timeout(
            STORAGE_IO_TIMEOUT,
            self.client
                .get_object()
                .bucket(&self.bucket)
                .key(self.key(key)?)
                .send(),
        )
        .await
        .context("S3 read timed out")??;
        Ok(timeout(STORAGE_IO_TIMEOUT, result.body.collect())
            .await
            .context("S3 response body timed out")??
            .into_bytes()
            .to_vec())
    }
    async fn delete(&self, key: &str) -> Result<()> {
        timeout(
            STORAGE_IO_TIMEOUT,
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(self.key(key)?)
                .send(),
        )
        .await
        .context("S3 delete timed out")??;
        Ok(())
    }
}

#[derive(Clone)]
struct StorageService {
    db: SqlitePool,
    encryption_key: [u8; 32],
    gate: Arc<tokio::sync::RwLock<()>>,
    source_deletion_gate: Arc<tokio::sync::Mutex<()>>,
    thumbnail_cache_dir: Arc<PathBuf>,
    thumbnail_locks: Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    thumbnail_maintenance_gate: Arc<tokio::sync::RwLock<()>>,
    cache: SharedStoreCache,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImageDerivative {
    Grid,
    Preview,
    High,
}

impl ImageDerivative {
    fn suffix(self) -> &'static str {
        match self {
            Self::Grid => "grid.png",
            Self::Preview => "preview.webp",
            Self::High => "high.webp",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            Self::Grid => "image/png",
            Self::Preview | Self::High => "image/webp",
        }
    }
}

type SharedStoreCache = Arc<tokio::sync::RwLock<Option<(String, Arc<dyn BlobStore>)>>>;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageSettingsInput {
    backend: String,
    local_path: Option<String>,
    webdav_url: Option<String>,
    webdav_username: Option<String>,
    webdav_password: Option<String>,
    webdav_prefix: Option<String>,
    s3_endpoint: Option<String>,
    s3_region: Option<String>,
    s3_bucket: Option<String>,
    s3_access_key: Option<String>,
    s3_secret_key: Option<String>,
    s3_prefix: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
struct StorageCandidate {
    backend: String,
    local_path: String,
    webdav_url: String,
    webdav_username: String,
    webdav_password: String,
    webdav_prefix: String,
    s3_endpoint: String,
    s3_region: String,
    s3_bucket: String,
    s3_access_key: String,
    s3_secret_key: String,
    s3_prefix: String,
}

impl StorageCandidate {
    fn fingerprint(&self) -> String {
        let serialized = format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
            self.backend,
            self.local_path,
            self.webdav_url,
            self.webdav_username,
            self.webdav_password,
            self.webdav_prefix,
            self.s3_endpoint,
            self.s3_region,
            self.s3_bucket,
            self.s3_access_key,
            self.s3_secret_key,
            self.s3_prefix
        );
        hex_digest(&Sha256::digest(serialized.as_bytes()))
    }
    fn validate(&self) -> Result<()> {
        if !matches!(self.backend.as_str(), "local" | "webdav" | "s3") {
            bail!("存储类型只能是 local、webdav 或 s3");
        }
        match self.backend.as_str() {
            "local" if self.local_path.is_empty() => bail!("本地存储路径不能为空"),
            "webdav" => {
                let parsed =
                    url::Url::parse(&self.webdav_url).context("WebDAV 地址必须是完整 URL")?;
                if !matches!(parsed.scheme(), "http" | "https")
                    || self.webdav_username.is_empty()
                    || self.webdav_password.is_empty()
                {
                    bail!("请填写有效的 WebDAV 地址、用户名和密码");
                }
                validate_storage_prefix(&self.webdav_prefix)?;
            }
            "s3" => {
                let parsed =
                    url::Url::parse(&self.s3_endpoint).context("S3 Endpoint 必须是完整 URL")?;
                if !matches!(parsed.scheme(), "http" | "https")
                    || self.s3_region.is_empty()
                    || self.s3_bucket.is_empty()
                    || self.s3_access_key.is_empty()
                    || self.s3_secret_key.is_empty()
                {
                    bail!("请填写 S3 Endpoint、区域、桶名、访问密钥和秘密访问密钥");
                }
                validate_storage_prefix(&self.s3_prefix)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn location_key(&self) -> String {
        match self.backend.as_str() {
            "local" => {
                let path = std::fs::canonicalize(&self.local_path)
                    .unwrap_or_else(|_| PathBuf::from(&self.local_path));
                format!("local:{}", path.to_string_lossy())
            }
            "webdav" => format!(
                "webdav:{}:{}",
                self.webdav_url.trim_end_matches('/'),
                self.webdav_prefix.trim_matches('/')
            ),
            "s3" => format!(
                "s3:{}:{}:{}:{}",
                self.s3_endpoint.trim_end_matches('/'),
                self.s3_region,
                self.s3_bucket,
                self.s3_prefix.trim_matches('/')
            ),
            _ => self.backend.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageSettingsOutput {
    backend: String,
    local_path: String,
    webdav_url: String,
    webdav_username: String,
    webdav_prefix: String,
    webdav_password_set: bool,
    s3_endpoint: String,
    s3_region: String,
    s3_bucket: String,
    s3_access_key: String,
    s3_secret_key_set: bool,
    s3_prefix: String,
}

const DEFAULT_SITE_TITLE: &str = "ChronoFrame";
const DEFAULT_SITE_SLOGAN: &str = "Frame the moments that matter.";
const DEFAULT_SITE_AUTHOR: &str = "ChronoFrame";
const DEFAULT_SITE_AVATAR_URL: &str = "/web-app-manifest-192x192.png";
const DEFAULT_SITE_THEME: &str = "system";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SiteSettings {
    title: String,
    slogan: String,
    author: String,
    avatar_url: String,
    theme: String,
}

impl SiteSettings {
    fn defaults() -> Self {
        Self {
            title: DEFAULT_SITE_TITLE.into(),
            slogan: DEFAULT_SITE_SLOGAN.into(),
            author: DEFAULT_SITE_AUTHOR.into(),
            avatar_url: DEFAULT_SITE_AVATAR_URL.into(),
            theme: DEFAULT_SITE_THEME.into(),
        }
    }

    fn normalize(self) -> ApiResult<Self> {
        let normalized = Self {
            title: self.title.trim().to_string(),
            slogan: self.slogan.trim().to_string(),
            author: self.author.trim().to_string(),
            avatar_url: self.avatar_url.trim().to_string(),
            theme: self.theme.trim().to_ascii_lowercase(),
        };
        if normalized.title.is_empty() || normalized.title.chars().count() > 100 {
            return Err(AppError::bad("网站名称不能为空且不得超过 100 个字符"));
        }
        if normalized.slogan.chars().count() > 200 {
            return Err(AppError::bad("网站标语不得超过 200 个字符"));
        }
        if normalized.author.chars().count() > 100 {
            return Err(AppError::bad("作者名称不得超过 100 个字符"));
        }
        if normalized.avatar_url.chars().count() > 2048 {
            return Err(AppError::bad("头像 URL 不得超过 2048 个字符"));
        }
        if !normalized.avatar_url.is_empty() {
            let valid_relative = normalized.avatar_url.starts_with('/')
                && !normalized.avatar_url.starts_with("//")
                && !normalized.avatar_url.chars().any(char::is_control);
            let valid_absolute = url::Url::parse(&normalized.avatar_url)
                .ok()
                .is_some_and(|url| matches!(url.scheme(), "http" | "https"));
            if !valid_relative && !valid_absolute {
                return Err(AppError::bad(
                    "头像 URL 必须是以 / 开头的站内路径，或完整的 HTTP(S) URL",
                ));
            }
        }
        if !matches!(normalized.theme.as_str(), "light" | "dark" | "system") {
            return Err(AppError::bad("默认主题只能是 light、dark 或 system"));
        }
        Ok(normalized)
    }
}

impl StorageService {
    fn new(db: SqlitePool, encryption_key: [u8; 32], thumbnail_cache_dir: PathBuf) -> Self {
        Self {
            db,
            encryption_key,
            gate: Arc::new(tokio::sync::RwLock::new(())),
            source_deletion_gate: Arc::new(tokio::sync::Mutex::new(())),
            thumbnail_cache_dir: Arc::new(thumbnail_cache_dir),
            thumbnail_locks: Arc::new(DashMap::new()),
            thumbnail_maintenance_gate: Arc::new(tokio::sync::RwLock::new(())),
            cache: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }
    fn derivative_path(&self, photo_id: &str, derivative: ImageDerivative) -> Result<PathBuf> {
        Uuid::parse_str(photo_id).context("缩略图图片 ID 无效")?;
        Ok(self
            .thumbnail_cache_dir
            .join(format!("{photo_id}.{}", derivative.suffix())))
    }
    #[cfg(test)]
    fn thumbnail_path(&self, photo_id: &str) -> Result<PathBuf> {
        self.derivative_path(photo_id, ImageDerivative::Grid)
    }
    fn legacy_thumbnail_paths(&self, photo_id: &str) -> Result<[PathBuf; 2]> {
        Uuid::parse_str(photo_id).context("缩略图图片 ID 无效")?;
        Ok([
            self.thumbnail_cache_dir.join(format!("{photo_id}.png")),
            self.thumbnail_cache_dir.join(format!("{photo_id}.webp")),
        ])
    }
    async fn cached_derivative(
        &self,
        photo_id: &str,
        derivative: ImageDerivative,
    ) -> Result<Option<Vec<u8>>> {
        let path = self.derivative_path(photo_id, derivative)?;
        match tokio::fs::read(path).await {
            Ok(data) => Ok(Some(data)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    #[cfg(test)]
    async fn cached_thumbnail(&self, photo_id: &str) -> Result<Option<Vec<u8>>> {
        self.cached_derivative(photo_id, ImageDerivative::Grid)
            .await
    }
    async fn cache_derivative(
        &self,
        photo_id: &str,
        derivative: ImageDerivative,
        data: &[u8],
    ) -> Result<()> {
        tokio::fs::create_dir_all(self.thumbnail_cache_dir.as_ref()).await?;
        let destination = self.derivative_path(photo_id, derivative)?;
        let temporary = self.thumbnail_cache_dir.join(format!(
            ".{photo_id}.{}.{}.tmp",
            derivative.suffix(),
            Uuid::new_v4()
        ));
        tokio::fs::write(&temporary, data).await?;
        if let Err(error) = tokio::fs::rename(&temporary, &destination).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            if error.kind() != ErrorKind::AlreadyExists {
                return Err(error.into());
            }
        }
        if derivative == ImageDerivative::Grid
            && let Ok(legacy_paths) = self.legacy_thumbnail_paths(photo_id)
        {
            for legacy in legacy_paths {
                if let Err(error) = tokio::fs::remove_file(legacy).await
                    && error.kind() != ErrorKind::NotFound
                {
                    warn!(photo_id, "failed to remove legacy thumbnail: {error:#}");
                }
            }
        }
        Ok(())
    }
    #[cfg(test)]
    async fn cache_thumbnail(&self, photo_id: &str, data: &[u8]) -> Result<()> {
        self.cache_derivative(photo_id, ImageDerivative::Grid, data)
            .await
    }
    async fn remove_cached_thumbnail(&self, photo_id: &str) {
        let mut paths = [
            ImageDerivative::Grid,
            ImageDerivative::Preview,
            ImageDerivative::High,
        ]
        .into_iter()
        .filter_map(|derivative| self.derivative_path(photo_id, derivative).ok())
        .collect::<Vec<_>>();
        if let Ok(legacy_paths) = self.legacy_thumbnail_paths(photo_id) {
            paths.extend(legacy_paths);
        }
        for path in paths {
            if let Err(error) = tokio::fs::remove_file(path).await
                && error.kind() != ErrorKind::NotFound
            {
                warn!(photo_id, "failed to remove cached thumbnail: {error:#}");
            }
        }
    }
    async fn clear_thumbnail_cache(&self) -> Result<usize> {
        // Wait for in-flight thumbnail writers and prevent new ones from entering while files are
        // removed. The cache directory is dedicated, so stale PNG/WEBP and interrupted temp files
        // can all be discarded safely.
        let _maintenance_guard = self.thumbnail_maintenance_gate.write().await;
        tokio::fs::create_dir_all(self.thumbnail_cache_dir.as_ref()).await?;
        let mut directory = tokio::fs::read_dir(self.thumbnail_cache_dir.as_ref()).await?;
        let mut paths = Vec::new();
        while let Some(entry) = directory.next_entry().await? {
            let file_type = entry.file_type().await?;
            if file_type.is_file() || file_type.is_symlink() {
                paths.push(entry.path());
            }
        }
        let removed = stream::iter(paths.into_iter().map(|path| async move {
            tokio::fs::remove_file(&path)
                .await
                .with_context(|| format!("无法删除缩略图缓存 {}", path.display()))?;
            Ok::<usize, anyhow::Error>(1)
        }))
        .buffer_unordered(64)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum();
        self.thumbnail_locks.clear();
        Ok(removed)
    }
    async fn values(&self) -> Result<HashMap<String, String>> {
        let rows = sqlx::query("SELECT key,value FROM app_settings WHERE key LIKE 'storage_%'")
            .fetch_all(&self.db)
            .await?;
        Ok(rows
            .iter()
            .map(|row| (row.get("key"), row.get("value")))
            .collect())
    }
    async fn public_settings(&self) -> Result<StorageSettingsOutput> {
        let values = self.values().await?;
        let get = |key: &str| values.get(key).cloned().unwrap_or_default();
        Ok(StorageSettingsOutput {
            backend: get("storage_backend"),
            local_path: get("storage_local_path"),
            webdav_url: get("storage_webdav_url"),
            webdav_username: get("storage_webdav_username"),
            webdav_prefix: get("storage_webdav_prefix"),
            webdav_password_set: !get("storage_webdav_password").is_empty(),
            s3_endpoint: get("storage_s3_endpoint"),
            s3_region: get("storage_s3_region"),
            s3_bucket: get("storage_s3_bucket"),
            s3_access_key: get("storage_s3_access_key"),
            s3_secret_key_set: !get("storage_s3_secret_key").is_empty(),
            s3_prefix: get("storage_s3_prefix"),
        })
    }
    async fn candidate_from_input(&self, input: &StorageSettingsInput) -> Result<StorageCandidate> {
        let values = self.values().await?;
        let backend = input.backend.trim().to_ascii_lowercase();
        let supplied_webdav_password = input
            .webdav_password
            .clone()
            .filter(|value| !value.is_empty());
        let supplied_s3_secret = input
            .s3_secret_key
            .clone()
            .filter(|value| !value.is_empty());
        let webdav_password = if backend == "webdav" {
            match supplied_webdav_password {
                Some(password) => password,
                None => values
                    .get("storage_webdav_password")
                    .filter(|value| !value.is_empty())
                    .map(|value| self.decrypt(value))
                    .transpose()?
                    .unwrap_or_default(),
            }
        } else {
            supplied_webdav_password.unwrap_or_default()
        };
        let s3_secret_key = if backend == "s3" {
            match supplied_s3_secret {
                Some(secret) => secret,
                None => values
                    .get("storage_s3_secret_key")
                    .filter(|value| !value.is_empty())
                    .map(|value| self.decrypt(value))
                    .transpose()?
                    .unwrap_or_default(),
            }
        } else {
            supplied_s3_secret.unwrap_or_default()
        };
        let candidate = StorageCandidate {
            backend,
            local_path: input
                .local_path
                .clone()
                .unwrap_or_else(|| "./data/storage".into())
                .trim()
                .to_string(),
            webdav_url: input
                .webdav_url
                .clone()
                .unwrap_or_default()
                .trim()
                .to_string(),
            webdav_username: input
                .webdav_username
                .clone()
                .unwrap_or_default()
                .trim()
                .to_string(),
            webdav_password,
            webdav_prefix: input
                .webdav_prefix
                .clone()
                .unwrap_or_else(|| "chronoframe".into())
                .trim_matches('/')
                .to_string(),
            s3_endpoint: input
                .s3_endpoint
                .clone()
                .unwrap_or_default()
                .trim()
                .to_string(),
            s3_region: input
                .s3_region
                .clone()
                .unwrap_or_else(|| "us-east-1".into())
                .trim()
                .to_string(),
            s3_bucket: input
                .s3_bucket
                .clone()
                .unwrap_or_default()
                .trim()
                .to_string(),
            s3_access_key: input
                .s3_access_key
                .clone()
                .unwrap_or_default()
                .trim()
                .to_string(),
            s3_secret_key,
            s3_prefix: input
                .s3_prefix
                .clone()
                .unwrap_or_else(|| "chronoframe".into())
                .trim_matches('/')
                .to_string(),
        };
        candidate.validate()?;
        Ok(candidate)
    }
    fn candidate_from_values(&self, values: &HashMap<String, String>) -> Result<StorageCandidate> {
        let get = |key: &str| values.get(key).cloned().unwrap_or_default();
        let backend = get("storage_backend");
        let candidate = StorageCandidate {
            backend: backend.clone(),
            local_path: get("storage_local_path"),
            webdav_url: get("storage_webdav_url"),
            webdav_username: get("storage_webdav_username"),
            webdav_password: if backend != "webdav" || get("storage_webdav_password").is_empty() {
                String::new()
            } else {
                self.decrypt(&get("storage_webdav_password"))
                    .context("无法解密 WebDAV 密码；请确认 storage master key 未被替换")?
            },
            webdav_prefix: get("storage_webdav_prefix"),
            s3_endpoint: get("storage_s3_endpoint"),
            s3_region: get("storage_s3_region"),
            s3_bucket: get("storage_s3_bucket"),
            s3_access_key: get("storage_s3_access_key"),
            s3_secret_key: if backend != "s3" || get("storage_s3_secret_key").is_empty() {
                String::new()
            } else {
                self.decrypt(&get("storage_s3_secret_key"))
                    .context("无法解密 S3 密钥；请确认 storage master key 未被替换")?
            },
            s3_prefix: get("storage_s3_prefix"),
        };
        candidate.validate()?;
        Ok(candidate)
    }
    async fn build_s3_store(candidate: &StorageCandidate) -> Result<Arc<S3Store>> {
        if candidate.backend != "s3" {
            bail!("当前活动存储不是 S3");
        }
        let shared = timeout(
            STORAGE_IO_TIMEOUT,
            aws_config::defaults(BehaviorVersion::latest())
                .region(Region::new(candidate.s3_region.clone()))
                .credentials_provider(Credentials::new(
                    candidate.s3_access_key.clone(),
                    candidate.s3_secret_key.clone(),
                    None,
                    None,
                    "chronoframe-settings",
                ))
                .endpoint_url(candidate.s3_endpoint.clone())
                .load(),
        )
        .await
        .context("S3 客户端初始化超时")?;
        let config = S3ConfigBuilder::from(&shared)
            .force_path_style(true)
            .build();
        Ok(Arc::new(S3Store {
            client: S3Client::from_conf(config),
            bucket: candidate.s3_bucket.clone(),
            prefix: candidate.s3_prefix.clone(),
        }))
    }
    async fn build_store(candidate: &StorageCandidate) -> Result<Arc<dyn BlobStore>> {
        match candidate.backend.as_str() {
            "local" => {
                let root = PathBuf::from(&candidate.local_path);
                tokio::fs::create_dir_all(&root).await?;
                Ok(Arc::new(LocalStore { root }))
            }
            "webdav" => {
                let mut base_url = url::Url::parse(&candidate.webdav_url)?;
                if !base_url.path().ends_with('/') {
                    base_url.set_path(&format!("{}/", base_url.path()));
                }
                let client = Client::builder()
                    .connect_timeout(Duration::from_secs(5))
                    .read_timeout(STORAGE_IO_TIMEOUT)
                    .timeout(STORAGE_IO_TIMEOUT)
                    .build()?;
                Ok(Arc::new(WebDavStore {
                    client,
                    base_url,
                    authorization: HeaderValue::from_str(&format!(
                        "Basic {}",
                        STANDARD.encode(format!(
                            "{}:{}",
                            candidate.webdav_username, candidate.webdav_password
                        ))
                    ))?,
                    prefix: candidate.webdav_prefix.clone(),
                }))
            }
            "s3" => {
                let store: Arc<dyn BlobStore> = Self::build_s3_store(candidate).await?;
                Ok(store)
            }
            value => bail!("未知存储配置：{value}"),
        }
    }
    async fn test_candidate(&self, input: &StorageSettingsInput) -> Result<()> {
        let candidate = self.candidate_from_input(input).await?;
        Self::build_store(&candidate).await?.healthcheck().await
    }
    async fn save(&self, input: StorageSettingsInput) -> Result<StorageSettingsOutput> {
        let _gate = self.gate.write().await;
        let candidate = self.candidate_from_input(&input).await?;
        let current = self.public_settings().await?;
        let target_changed = current.backend != candidate.backend
            || match candidate.backend.as_str() {
                "local" => current.local_path != candidate.local_path,
                "webdav" => {
                    current.webdav_url != candidate.webdav_url
                        || current.webdav_prefix != candidate.webdav_prefix
                }
                "s3" => {
                    current.s3_endpoint != candidate.s3_endpoint
                        || current.s3_region != candidate.s3_region
                        || current.s3_bucket != candidate.s3_bucket
                        || current.s3_prefix != candidate.s3_prefix
                }
                _ => true,
            };
        if target_changed {
            let stored_count: i64 = sqlx::query_scalar(
                "SELECT (SELECT COUNT(*) FROM photos) + (SELECT COUNT(*) FROM pending_blobs) + (SELECT COUNT(*) FROM photo_deletion_outbox) + (SELECT COUNT(*) FROM source_deletion_outbox)",
            )
            .fetch_one(&self.db)
            .await?;
            if stored_count > 0 {
                bail!("已有图片或待清理对象时不能直接切换存储目标；请先完成存储迁移或等待清理完成");
            }
        }
        Self::build_store(&candidate)
            .await?
            .healthcheck()
            .await
            .context("存储连接测试失败")?;
        let mut tx = self.db.begin().await?;
        for (key, value) in [
            ("storage_backend", candidate.backend.clone()),
            ("storage_local_path", candidate.local_path.clone()),
            ("storage_webdav_url", candidate.webdav_url.clone()),
            ("storage_webdav_username", candidate.webdav_username.clone()),
            ("storage_webdav_prefix", candidate.webdav_prefix.clone()),
            ("storage_s3_endpoint", candidate.s3_endpoint.clone()),
            ("storage_s3_region", candidate.s3_region.clone()),
            ("storage_s3_bucket", candidate.s3_bucket.clone()),
            ("storage_s3_access_key", candidate.s3_access_key.clone()),
            ("storage_s3_prefix", candidate.s3_prefix.clone()),
        ] {
            sqlx::query("INSERT INTO app_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(key).bind(value).execute(&mut *tx).await?;
        }
        if let Some(password) = input.webdav_password.filter(|value| !value.is_empty()) {
            let encrypted = self.encrypt(&password)?;
            sqlx::query("INSERT INTO app_settings(key,value) VALUES('storage_webdav_password',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(encrypted).execute(&mut *tx).await?;
        }
        if let Some(secret) = input.s3_secret_key.filter(|value| !value.is_empty()) {
            let encrypted = self.encrypt(&secret)?;
            sqlx::query("INSERT INTO app_settings(key,value) VALUES('storage_s3_secret_key',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(encrypted).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        *self.cache.write().await = None;
        self.public_settings().await
    }
    async fn current_candidate(&self) -> Result<StorageCandidate> {
        self.candidate_from_values(&self.values().await?)
    }
    async fn activate_migration_candidate(
        &self,
        job_id: &str,
        candidate: &StorageCandidate,
    ) -> Result<()> {
        let _gate = self.gate.write().await;
        let encrypted_webdav_password = if candidate.webdav_password.is_empty() {
            String::new()
        } else {
            self.encrypt(&candidate.webdav_password)?
        };
        let encrypted_s3_secret = if candidate.s3_secret_key.is_empty() {
            String::new()
        } else {
            self.encrypt(&candidate.s3_secret_key)?
        };
        let mut tx = self.db.begin().await?;
        for (key, value) in [
            ("storage_backend", candidate.backend.clone()),
            ("storage_local_path", candidate.local_path.clone()),
            ("storage_webdav_url", candidate.webdav_url.clone()),
            ("storage_webdav_username", candidate.webdav_username.clone()),
            ("storage_webdav_prefix", candidate.webdav_prefix.clone()),
            ("storage_webdav_password", encrypted_webdav_password),
            ("storage_s3_endpoint", candidate.s3_endpoint.clone()),
            ("storage_s3_region", candidate.s3_region.clone()),
            ("storage_s3_bucket", candidate.s3_bucket.clone()),
            ("storage_s3_access_key", candidate.s3_access_key.clone()),
            ("storage_s3_secret_key", encrypted_s3_secret),
            ("storage_s3_prefix", candidate.s3_prefix.clone()),
        ] {
            sqlx::query("INSERT INTO app_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
                .bind(key)
                .bind(value)
                .execute(&mut *tx)
                .await?;
        }
        let changed = sqlx::query("UPDATE storage_migration_jobs SET status='completed',cleanup_status='pending',activated_at=?,updated_at=?,error=NULL WHERE id=? AND status='running'")
            .bind(now())
            .bind(now())
            .bind(job_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if changed != 1 {
            bail!("迁移任务状态已经改变，拒绝切换存储");
        }
        tx.commit().await?;
        *self.cache.write().await = None;
        Ok(())
    }
    async fn store(&self) -> Result<Arc<dyn BlobStore>> {
        let candidate = self.candidate_from_values(&self.values().await?)?;
        let fingerprint = candidate.fingerprint();
        if let Some((cached_fingerprint, cached)) = self.cache.read().await.as_ref()
            && cached_fingerprint == &fingerprint
        {
            return Ok(cached.clone());
        }
        let built = Self::build_store(&candidate).await?;
        let mut cache = self.cache.write().await;
        if let Some((cached_fingerprint, cached)) = cache.as_ref()
            && cached_fingerprint == &fingerprint
        {
            return Ok(cached.clone());
        }
        *cache = Some((fingerprint, built.clone()));
        Ok(built)
    }
    fn encrypt(&self, plaintext: &str) -> Result<String> {
        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)?;
        let nonce = Aes256Gcm::generate_nonce(&mut AeadOsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| anyhow!("无法加密 WebDAV 密码"))?;
        let mut payload = nonce.to_vec();
        payload.extend(ciphertext);
        Ok(STANDARD.encode(payload))
    }
    fn decrypt(&self, encoded: &str) -> Result<String> {
        let payload = STANDARD.decode(encoded)?;
        if payload.len() < 13 {
            bail!("密文无效");
        }
        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)?;
        let plaintext = cipher
            .decrypt(aes_gcm::Nonce::from_slice(&payload[..12]), &payload[12..])
            .map_err(|_| anyhow!("密文无效"))?;
        Ok(String::from_utf8(plaintext)?)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Album {
    id: String,
    name: String,
    description: String,
    created_at: i64,
    display_created_date: Option<String>,
    photo_date_start: Option<String>,
    photo_date_end: Option<String>,
    position: i64,
    photo_count: i64,
    #[serde(flatten)]
    cover: album_covers::Cover,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Photo {
    id: String,
    album_id: String,
    original_name: String,
    storage_key: String,
    format: String,
    content_type: String,
    byte_size: i64,
    width: i64,
    height: i64,
    created_at: i64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlbumDetail {
    #[serde(flatten)]
    album: Album,
    photos: Vec<Photo>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConversionJob {
    id: String,
    status: String,
    target_format: String,
    total: i64,
    completed: i64,
    succeeded: i64,
    failed: i64,
    cancelled: i64,
    created_at: i64,
    updated_at: i64,
    sources_deleted_at: Option<i64>,
    source_delete_total: i64,
    source_delete_completed: i64,
    source_delete_remaining: i64,
    source_delete_failed: i64,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConversionItem {
    id: String,
    source_photo_id: String,
    source_name: String,
    status: String,
    target_photo_id: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorageMigrationJob {
    id: String,
    status: String,
    source_backend: String,
    target_backend: String,
    total: i64,
    completed: i64,
    succeeded: i64,
    failed: i64,
    cancelled: i64,
    cleanup_status: String,
    cleanup_completed: i64,
    cleanup_failed: i64,
    created_at: i64,
    updated_at: i64,
    activated_at: Option<i64>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThumbnailRebuildJob {
    id: String,
    status: String,
    phase: String,
    total: i64,
    completed: i64,
    succeeded: i64,
    failed: i64,
    skipped: i64,
    cancelled: i64,
    cache_files_removed: i64,
    worker_count: i64,
    created_at: i64,
    updated_at: i64,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct S3CleanupJob {
    id: String,
    status: String,
    phase: String,
    scanned_objects: i64,
    protected_objects: i64,
    total: i64,
    completed: i64,
    deleted: i64,
    failed: i64,
    skipped: i64,
    bytes_found: i64,
    bytes_deleted: i64,
    worker_count: i64,
    managed_prefix: String,
    created_at: i64,
    updated_at: i64,
    error: Option<String>,
}

fn album_from(row: &sqlx::sqlite::SqliteRow) -> Album {
    Album {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        created_at: row.get("created_at"),
        display_created_date: row.get("display_created_date"),
        photo_date_start: row.get("photo_date_start"),
        photo_date_end: row.get("photo_date_end"),
        position: row.get("position"),
        photo_count: row.get("photo_count"),
        cover: album_covers::from_row(row),
    }
}
fn photo_from(row: &sqlx::sqlite::SqliteRow) -> Photo {
    Photo {
        id: row.get("id"),
        album_id: row.get("album_id"),
        original_name: row.get("original_name"),
        storage_key: row.get("storage_key"),
        format: row.get("format"),
        content_type: row.get("content_type"),
        byte_size: row.get("byte_size"),
        width: row.get("width"),
        height: row.get("height"),
        created_at: row.get("created_at"),
    }
}
fn job_from(row: &sqlx::sqlite::SqliteRow) -> ConversionJob {
    ConversionJob {
        id: row.get("id"),
        status: row.get("status"),
        target_format: row.get("target_format"),
        total: row.get("total"),
        completed: row.get("completed"),
        succeeded: row.get("succeeded"),
        failed: row.get("failed"),
        cancelled: row.get("cancelled"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        sources_deleted_at: row.get("sources_deleted_at"),
        source_delete_total: row.get("source_delete_total"),
        source_delete_completed: row.get("source_delete_completed"),
        source_delete_remaining: row.get("source_delete_remaining"),
        source_delete_failed: row.get("source_delete_failed"),
    }
}
fn migration_job_from(row: &sqlx::sqlite::SqliteRow) -> StorageMigrationJob {
    StorageMigrationJob {
        id: row.get("id"),
        status: row.get("status"),
        source_backend: row.get("source_backend"),
        target_backend: row.get("target_backend"),
        total: row.get("total"),
        completed: row.get("completed"),
        succeeded: row.get("succeeded"),
        failed: row.get("failed"),
        cancelled: row.get("cancelled"),
        cleanup_status: row.get("cleanup_status"),
        cleanup_completed: row.get("cleanup_completed"),
        cleanup_failed: row.get("cleanup_failed"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        activated_at: row.get("activated_at"),
        error: row.get("error"),
    }
}
fn thumbnail_job_from(row: &sqlx::sqlite::SqliteRow) -> ThumbnailRebuildJob {
    ThumbnailRebuildJob {
        id: row.get("id"),
        status: row.get("status"),
        phase: row.get("phase"),
        total: row.get("total"),
        completed: row.get("completed"),
        succeeded: row.get("succeeded"),
        failed: row.get("failed"),
        skipped: row.get("skipped"),
        cancelled: row.get("cancelled"),
        cache_files_removed: row.get("cache_files_removed"),
        worker_count: row.get("worker_count"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        error: row.get("error"),
    }
}
fn s3_cleanup_job_from(row: &sqlx::sqlite::SqliteRow) -> S3CleanupJob {
    S3CleanupJob {
        id: row.get("id"),
        status: row.get("status"),
        phase: row.get("phase"),
        scanned_objects: row.get("scanned_objects"),
        protected_objects: row.get("protected_objects"),
        total: row.get("total"),
        completed: row.get("completed"),
        deleted: row.get("deleted"),
        failed: row.get("failed"),
        skipped: row.get("skipped"),
        bytes_found: row.get("bytes_found"),
        bytes_deleted: row.get("bytes_deleted"),
        worker_count: row.get("worker_count"),
        managed_prefix: row.get("managed_prefix"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        error: row.get("error"),
    }
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

async fn register_pending_blob(db: &SqlitePool, key: &str) -> Result<()> {
    sqlx::query("INSERT INTO pending_blobs(key,created_at) VALUES(?,?)")
        .bind(key)
        .bind(now())
        .execute(db)
        .await?;
    Ok(())
}

async fn cleanup_pending_blob(
    store: &Arc<dyn BlobStore>,
    db: &SqlitePool,
    key: &str,
) -> Result<()> {
    store
        .delete(&staging_key(key))
        .await
        .context("清理暂存对象失败")?;
    store.delete(key).await.context("清理未提交对象失败")?;
    sqlx::query("DELETE FROM pending_blobs WHERE key=?")
        .bind(key)
        .execute(db)
        .await?;
    Ok(())
}

async fn recover_pending_blobs(
    storage: &StorageService,
    db: &SqlitePool,
    startup: bool,
) -> Result<usize> {
    // Cleanup is exclusive with uploads/conversions. Otherwise a janitor could see the ledger
    // before the photo transaction commits and delete an object that is still being uploaded.
    let _guard = storage.gate.write().await;
    let settings = storage.public_settings().await?;
    let grace = if startup && settings.backend == "local" {
        0
    } else {
        STORAGE_IO_TIMEOUT.as_secs() as i64 * 2
    };
    let rows = sqlx::query("SELECT key FROM pending_blobs WHERE created_at<=? ORDER BY created_at")
        .bind(now() - grace)
        .fetch_all(db)
        .await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let store = storage.store().await?;
    let mut cleaned = 0;
    for row in rows {
        let key: String = row.get("key");
        let referenced: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM photos WHERE storage_key=?)")
                .bind(&key)
                .fetch_one(db)
                .await?;
        let result = if referenced {
            match store.delete(&staging_key(&key)).await {
                Ok(()) => sqlx::query("DELETE FROM pending_blobs WHERE key=?")
                    .bind(&key)
                    .execute(db)
                    .await
                    .map(|_| ())
                    .map_err(Into::into),
                Err(error) => Err(error),
            }
        } else {
            cleanup_pending_blob(&store, db, &key).await
        };
        match result {
            Ok(()) => cleaned += 1,
            Err(error) => warn!(storage_key = %key, "pending blob cleanup deferred: {error:#}"),
        }
    }
    Ok(cleaned)
}

#[derive(Default)]
struct PhotoDeletionDrain {
    removed: usize,
    failures: Vec<serde_json::Value>,
}

async fn drain_photo_deletion_outbox(
    storage: &StorageService,
    db: &SqlitePool,
) -> Result<PhotoDeletionDrain> {
    let rows = sqlx::query(
        "SELECT photo_id,storage_key FROM photo_deletion_outbox ORDER BY created_at,photo_id",
    )
    .fetch_all(db)
    .await?;
    let store = if rows.is_empty() {
        None
    } else {
        Some(storage.store().await?)
    };
    let mut result = PhotoDeletionDrain::default();
    for row in rows {
        let photo_id: String = row.get("photo_id");
        let storage_key: String = row.get("storage_key");
        let store = store.as_ref().expect("outbox rows require a store");
        if let Err(error) = store.delete(&storage_key).await {
            result
                .failures
                .push(serde_json::json!({"photoId": photo_id, "error": error.to_string()}));
            continue;
        }
        sqlx::query("DELETE FROM photo_deletion_outbox WHERE photo_id=?")
            .bind(&photo_id)
            .execute(db)
            .await?;
        storage.remove_cached_thumbnail(&photo_id).await;
        result.removed += 1;
    }
    Ok(result)
}

#[derive(Default)]
struct SourceDeletionDrain {
    removed: usize,
    failures: Vec<serde_json::Value>,
}

async fn verify_target_blob(
    store: &Arc<dyn BlobStore>,
    key: &str,
    format: &str,
    expected_size: i64,
) -> Result<()> {
    let data = timeout(STORAGE_IO_TIMEOUT, store.get(key))
        .await
        .context("验证转换图读取超时")??;
    if data.len() as i64 != expected_size {
        bail!("转换图大小校验失败");
    }
    let detected = match image::guess_format(&data).context("转换图格式无法识别")? {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::WebP => "webp",
        _ => "unsupported",
    };
    if detected != format {
        bail!("转换图格式校验失败");
    }
    tokio::task::spawn_blocking(move || image::load_from_memory(&data).map(|_| ()))
        .await
        .context("转换图校验线程异常")??;
    Ok(())
}

async fn drain_source_deletion_outbox(
    storage: &StorageService,
    db: &SqlitePool,
    only_job: Option<&str>,
) -> Result<SourceDeletionDrain> {
    // Only one drain owns a snapshot of the durable outbox at a time. Items inside that
    // snapshot still run concurrently, so a janitor retry can never race an API-triggered run.
    let _drain_guard = storage.source_deletion_gate.lock().await;
    let rows = if let Some(job_id) = only_job {
        sqlx::query("SELECT job_id,source_photo_id,source_key,target_key,target_format,target_size,attempts FROM source_deletion_outbox WHERE job_id=? AND next_retry_at<=? ORDER BY created_at,source_photo_id").bind(job_id).bind(now()).fetch_all(db).await?
    } else {
        sqlx::query("SELECT job_id,source_photo_id,source_key,target_key,target_format,target_size,attempts FROM source_deletion_outbox WHERE next_retry_at<=? ORDER BY created_at,source_photo_id").bind(now()).fetch_all(db).await?
    };
    let store = if rows.is_empty() {
        None
    } else {
        Some(storage.store().await?)
    };
    let outcomes = if let Some(store) = store {
        let db = db.clone();
        let storage = storage.clone();
        stream::iter(rows.into_iter().map(|row| {
            let db = db.clone();
            let storage = storage.clone();
            let store = store.clone();
            async move {
                let job_id: String = row.get("job_id");
                let source_photo_id: String = row.get("source_photo_id");
                let source_key: String = row.get("source_key");
                let target_key: String = row.get("target_key");
                let target_format: String = row.get("target_format");
                let target_size: i64 = row.get("target_size");
                let attempts: i64 = row.get("attempts");
                let outcome: Result<()> = async {
                    verify_target_blob(&store, &target_key, &target_format, target_size)
                        .await
                        .context("目标图验证失败")?;
                    store.delete(&source_key).await.context("旧图对象删除失败")?;
                    let mut tx = db.begin().await?;
                    sqlx::query("DELETE FROM photos WHERE id=?")
                        .bind(&source_photo_id)
                        .execute(&mut *tx)
                        .await?;
                    sqlx::query("DELETE FROM source_deletion_outbox WHERE job_id=? AND source_photo_id=?")
                        .bind(&job_id)
                        .bind(&source_photo_id)
                        .execute(&mut *tx)
                        .await?;
                    tx.commit().await?;
                    storage.remove_cached_thumbnail(&source_photo_id).await;
                    Ok(())
                }
                .await;
                match outcome {
                    Ok(()) => Ok(source_photo_id),
                    Err(error) => {
                        let message = format!("{error:#}");
                        let retry_delay = 5_i64 * 2_i64.pow((attempts + 1).clamp(0, 6) as u32);
                        if let Err(update_error) = sqlx::query("UPDATE source_deletion_outbox SET attempts=attempts+1,last_error=?,next_retry_at=? WHERE job_id=? AND source_photo_id=?")
                            .bind(&message)
                            .bind(now() + retry_delay)
                            .bind(&job_id)
                            .bind(&source_photo_id)
                            .execute(&db)
                            .await
                        {
                            warn!(job_id, photo_id = %source_photo_id, "failed to persist source deletion retry: {update_error:#}");
                        }
                        Err(serde_json::json!({
                            "photoId": source_photo_id,
                            "error": message
                        }))
                    }
                }
            }
        }))
        .buffer_unordered(DEFAULT_SOURCE_DELETE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
    } else {
        vec![]
    };
    let mut result = SourceDeletionDrain::default();
    for outcome in outcomes {
        match outcome {
            Ok(_) => result.removed += 1,
            Err(failure) => result.failures.push(failure),
        }
    }
    if let Some(job_id) = only_job {
        sqlx::query("UPDATE conversion_jobs SET sources_deleted_at=?,updated_at=? WHERE id=? AND sources_deleted_at=-2 AND NOT EXISTS(SELECT 1 FROM source_deletion_outbox WHERE job_id=?)").bind(now()).bind(now()).bind(job_id).bind(job_id).execute(db).await?;
    } else {
        sqlx::query("UPDATE conversion_jobs SET sources_deleted_at=?,updated_at=? WHERE sources_deleted_at=-2 AND NOT EXISTS(SELECT 1 FROM source_deletion_outbox WHERE job_id=conversion_jobs.id)").bind(now()).bind(now()).execute(db).await?;
    }
    Ok(result)
}

#[derive(Debug)]
struct AppError {
    status: StatusCode,
    message: String,
    clear_auth_cookies: Option<bool>,
}
impl AppError {
    fn bad(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            clear_auth_cookies: None,
        }
    }
    fn internal(e: impl Into<anyhow::Error>) -> Self {
        let e = e.into();
        error!("{e:#}");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "服务器处理请求时出错".into(),
            clear_auth_cookies: None,
        }
    }
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
            clear_auth_cookies: None,
        }
    }
    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
            clear_auth_cookies: None,
        }
    }
    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
            clear_auth_cookies: None,
        }
    }
    fn too_many(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: message.into(),
            clear_auth_cookies: None,
        }
    }
    fn clearing_auth_cookies(mut self, secure: bool) -> Self {
        self.clear_auth_cookies = Some(secure);
        self
    }
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let AppError {
            status,
            message,
            clear_auth_cookies,
        } = self;
        let mut response = (status, Json(serde_json::json!({"error": message}))).into_response();
        if let Some(secure) = clear_auth_cookies {
            for cookie in [
                expired_cookie(SESSION_COOKIE, true, secure),
                expired_cookie(CSRF_COOKIE, false, secure),
            ] {
                if let Ok(value) = HeaderValue::from_str(&cookie) {
                    response.headers_mut().append(header::SET_COOKIE, value);
                }
            }
            no_store(&mut response);
        }
        response
    }
}
type ApiResult<T> = std::result::Result<T, AppError>;

async fn api_not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "API 路径不存在"})),
    )
}

#[derive(Deserialize)]
struct CredentialsInput {
    username: String,
    password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthStatus {
    initialized: bool,
    authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
}

struct SessionRecord {
    token_hash: String,
    csrf_hash: String,
    username: String,
}

struct FreshSession {
    token: String,
    csrf: String,
    token_hash: String,
    csrf_hash: String,
    created_at: i64,
    expires_at: i64,
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|part| {
            let (candidate, value) = part.trim().split_once('=')?;
            (candidate == name).then(|| value.to_string())
        })
}

fn digest_secret(secret: &str) -> String {
    hex_digest(&Sha256::digest(secret.as_bytes()))
}

fn secrets_equal(left: &str, right: &str) -> bool {
    let left_hash = Sha256::digest(left.as_bytes());
    let right_hash = Sha256::digest(right.as_bytes());
    bool::from(left_hash[..].ct_eq(&right_hash[..]))
}

fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn fresh_session() -> FreshSession {
    let token = random_secret();
    let csrf = random_secret();
    let created_at = now();
    FreshSession {
        token_hash: digest_secret(&token),
        csrf_hash: digest_secret(&csrf),
        token,
        csrf,
        created_at,
        expires_at: created_at + SESSION_TTL_SECONDS,
    }
}

fn request_is_https(headers: &HeaderMap, state: &AppState) -> bool {
    if !state.trust_proxy_headers {
        return false;
    }
    let forwarded_https = headers
        .get("forwarded")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split([',', ';']).any(|part| {
                part.trim().split_once('=').is_some_and(|(key, value)| {
                    key.trim().eq_ignore_ascii_case("proto")
                        && value.trim().trim_matches('"').eq_ignore_ascii_case("https")
                })
            })
        });
    let forwarded_proto_https = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("https"));
    forwarded_https || forwarded_proto_https
}

fn use_secure_cookies(headers: &HeaderMap, state: &AppState) -> bool {
    state
        .secure_cookies
        .unwrap_or_else(|| request_is_https(headers, state))
}

fn cookie_suffix(secure: bool) -> &'static str {
    if secure { "; Secure" } else { "" }
}

fn session_cookie(token: &str, secure: bool) -> String {
    format!(
        "{SESSION_COOKIE}={token}; Path=/; Max-Age={SESSION_TTL_SECONDS}; HttpOnly; SameSite=Strict{}",
        cookie_suffix(secure)
    )
}

fn csrf_cookie(token: &str, secure: bool) -> String {
    format!(
        "{CSRF_COOKIE}={token}; Path=/; Max-Age={SESSION_TTL_SECONDS}; SameSite=Strict{}",
        cookie_suffix(secure)
    )
}

fn expired_cookie(name: &str, http_only: bool, secure: bool) -> String {
    format!(
        "{name}=; Path=/; Max-Age=0; SameSite=Strict{}{}",
        if http_only { "; HttpOnly" } else { "" },
        cookie_suffix(secure)
    )
}

fn append_cookie(headers: &mut HeaderMap, cookie: String) -> ApiResult<()> {
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(AppError::internal)?,
    );
    Ok(())
}

fn no_store(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
}

fn require_requested_with(headers: &HeaderMap) -> ApiResult<()> {
    if headers
        .get("x-requested-with")
        .and_then(|value| value.to_str().ok())
        == Some(REQUESTED_WITH)
    {
        Ok(())
    } else {
        Err(AppError::forbidden("缺少有效的 X-Requested-With 请求头"))
    }
}

async fn admin_initialized(db: &SqlitePool) -> ApiResult<bool> {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM administrators WHERE id=1)")
        .fetch_one(db)
        .await
        .map_err(AppError::internal)
}

async fn find_session(headers: &HeaderMap, state: &AppState) -> ApiResult<Option<SessionRecord>> {
    let Some(token) = cookie_value(headers, SESSION_COOKIE) else {
        return Ok(None);
    };
    let token_hash = digest_secret(&token);
    let row = sqlx::query("SELECT a.username,s.csrf_hash,s.expires_at FROM admin_sessions s JOIN administrators a ON a.id=s.administrator_id WHERE s.token_hash=?")
        .bind(&token_hash)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.get::<i64, _>("expires_at") <= now() {
        sqlx::query("DELETE FROM admin_sessions WHERE token_hash=?")
            .bind(&token_hash)
            .execute(&state.db)
            .await
            .map_err(AppError::internal)?;
        return Ok(None);
    }
    Ok(Some(SessionRecord {
        token_hash,
        csrf_hash: row.get("csrf_hash"),
        username: row.get("username"),
    }))
}

async fn require_admin(
    headers: &HeaderMap,
    state: &AppState,
    require_csrf: bool,
) -> ApiResult<SessionRecord> {
    let session = match find_session(headers, state).await? {
        Some(session) => session,
        None => {
            let error = AppError::unauthorized("请先登录管理员账号");
            return Err(if cookie_value(headers, SESSION_COOKIE).is_some() {
                error.clearing_auth_cookies(use_secure_cookies(headers, state))
            } else {
                error
            });
        }
    };
    if require_csrf {
        let cookie = cookie_value(headers, CSRF_COOKIE)
            .ok_or_else(|| AppError::forbidden("CSRF 校验失败"))?;
        let supplied = headers
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| AppError::forbidden("CSRF 校验失败"))?;
        let supplied_hash = digest_secret(supplied);
        if !secrets_equal(&cookie, supplied) || !secrets_equal(&session.csrf_hash, &supplied_hash) {
            return Err(AppError::forbidden("CSRF 校验失败"));
        }
    }
    Ok(session)
}

fn configured_argon2() -> Result<Argon2<'static>> {
    let params = Params::new(19_456, 2, 1, None)
        .map_err(|error| anyhow!("invalid Argon2 parameters: {error}"))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

fn hash_password(password: String) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    configured_argon2()?
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow!("failed to hash password: {error}"))
}

fn verify_password(password: String, encoded: String) -> Result<bool> {
    let hash = PasswordHash::new(&encoded)
        .map_err(|error| anyhow!("stored password hash is invalid: {error}"))?;
    Ok(configured_argon2()?
        .verify_password(password.as_bytes(), &hash)
        .is_ok())
}

async fn verify_password_bounded(
    state: &AppState,
    password: String,
    encoded: String,
) -> ApiResult<bool> {
    let _permit = timeout(Duration::from_secs(2), state.password_hash_slots.acquire())
        .await
        .map_err(|_| AppError::too_many("登录请求过多，请稍后重试"))?
        .map_err(|_| AppError::internal(anyhow!("password hash queue closed")))?;
    tokio::task::spawn_blocking(move || verify_password(password, encoded))
        .await
        .map_err(AppError::internal)?
        .map_err(AppError::internal)
}

async fn create_session(state: &AppState) -> ApiResult<(String, String)> {
    let session = fresh_session();
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    sqlx::query("DELETE FROM admin_sessions WHERE expires_at<=?")
        .bind(session.created_at)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    sqlx::query("INSERT INTO admin_sessions(token_hash,administrator_id,csrf_hash,created_at,expires_at) VALUES(?,1,?,?,?)")
        .bind(&session.token_hash)
        .bind(&session.csrf_hash)
        .bind(session.created_at)
        .bind(session.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    sqlx::query("DELETE FROM admin_sessions WHERE administrator_id=1 AND token_hash NOT IN (SELECT token_hash FROM admin_sessions WHERE administrator_id=1 ORDER BY created_at DESC,rowid DESC LIMIT 16)")
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    tx.commit().await.map_err(AppError::internal)?;
    Ok((session.token, session.csrf))
}

fn validate_credentials(input: CredentialsInput) -> ApiResult<(String, String)> {
    let username = input.username.trim().to_string();
    if username.is_empty()
        || username.chars().count() > 64
        || username.chars().any(char::is_control)
    {
        return Err(AppError::bad(
            "用户名不能为空、不得超过 64 个字符且不能包含控制字符",
        ));
    }
    if input.password.chars().count() < 12 || input.password.len() > 1024 {
        return Err(AppError::bad("密码至少需要 12 个字符且不得超过 1024 字节"));
    }
    Ok((username, input.password))
}

fn auth_json_error(error: JsonRejection) -> AppError {
    AppError {
        status: error.status(),
        message: "认证请求 JSON 无效或超过 8KiB 限制".into(),
        clear_auth_cookies: None,
    }
}

fn auth_body_error(error: BytesRejection) -> AppError {
    AppError {
        status: error.status(),
        message: "认证请求体超过 8KiB 限制".into(),
        clear_auth_cookies: None,
    }
}

async fn rotate_csrf_if_current(
    db: &SqlitePool,
    token_hash: &str,
    expected_hash: &str,
    replacement_hash: &str,
) -> ApiResult<bool> {
    let result =
        sqlx::query("UPDATE admin_sessions SET csrf_hash=? WHERE token_hash=? AND csrf_hash=?")
            .bind(replacement_hash)
            .bind(token_hash)
            .bind(expected_hash)
            .execute(db)
            .await
            .map_err(AppError::internal)?;
    Ok(result.rows_affected() == 1)
}

fn authenticated_response(
    status: StatusCode,
    username: String,
    session: &str,
    csrf: &str,
    secure: bool,
) -> ApiResult<Response> {
    let mut response = (
        status,
        Json(AuthStatus {
            initialized: true,
            authenticated: true,
            username: Some(username),
        }),
    )
        .into_response();
    append_cookie(response.headers_mut(), session_cookie(session, secure))?;
    append_cookie(response.headers_mut(), csrf_cookie(csrf, secure))?;
    no_store(&mut response);
    Ok(response)
}

async fn auth_status(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let initialized = admin_initialized(&state.db).await?;
    let session = if initialized {
        find_session(&headers, &state).await?
    } else {
        None
    };
    let authenticated = session.is_some();
    let username = session.as_ref().map(|session| session.username.clone());
    let mut response = Json(AuthStatus {
        initialized,
        authenticated,
        username,
    })
    .into_response();
    let secure = use_secure_cookies(&headers, &state);
    if let Some(session) = session {
        let csrf_matches = cookie_value(&headers, CSRF_COOKIE)
            .is_some_and(|csrf| secrets_equal(&session.csrf_hash, &digest_secret(&csrf)));
        if !csrf_matches {
            let csrf = random_secret();
            let replacement_hash = digest_secret(&csrf);
            if rotate_csrf_if_current(
                &state.db,
                &session.token_hash,
                &session.csrf_hash,
                &replacement_hash,
            )
            .await?
            {
                append_cookie(response.headers_mut(), csrf_cookie(&csrf, secure))?;
            }
        }
    } else if cookie_value(&headers, SESSION_COOKIE).is_some()
        || cookie_value(&headers, CSRF_COOKIE).is_some()
    {
        append_cookie(
            response.headers_mut(),
            expired_cookie(SESSION_COOKIE, true, secure),
        )?;
        append_cookie(
            response.headers_mut(),
            expired_cookie(CSRF_COOKIE, false, secure),
        )?;
    }
    no_store(&mut response);
    Ok(response)
}

async fn register_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: std::result::Result<Json<CredentialsInput>, JsonRejection>,
) -> ApiResult<Response> {
    require_requested_with(&headers)?;
    let Json(input) = input.map_err(auth_json_error)?;
    if admin_initialized(&state.db).await? {
        return Err(AppError::conflict("管理员账号已经注册"));
    }
    let (username, password) = validate_credentials(input)?;
    // Keep the bounded permit through hashing and the transaction. On queue timeout, distinguish
    // a completed competing registration (409) from genuine admission pressure (429).
    let _permit = match timeout(Duration::from_secs(2), state.password_hash_slots.acquire()).await {
        Ok(Ok(permit)) => permit,
        Ok(Err(_)) => {
            return Err(AppError::internal(anyhow!("password hash queue closed")));
        }
        Err(_) => {
            return Err(if admin_initialized(&state.db).await? {
                AppError::conflict("管理员账号已经被其他请求注册")
            } else {
                AppError::too_many("注册请求过多，请稍后重试")
            });
        }
    };
    if admin_initialized(&state.db).await? {
        return Err(AppError::conflict("管理员账号已经被其他请求注册"));
    }
    let password_hash = tokio::task::spawn_blocking(move || hash_password(password))
        .await
        .map_err(AppError::internal)?
        .map_err(AppError::internal)?;
    let session = fresh_session();
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    let result = sqlx::query("INSERT INTO administrators(id,username,password_hash,created_at) VALUES(1,?,?,?) ON CONFLICT(id) DO NOTHING")
        .bind(&username)
        .bind(password_hash)
        .bind(session.created_at)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    if result.rows_affected() != 1 {
        tx.rollback().await.map_err(AppError::internal)?;
        return Err(AppError::conflict("管理员账号已经被其他请求注册"));
    }
    sqlx::query("INSERT INTO admin_sessions(token_hash,administrator_id,csrf_hash,created_at,expires_at) VALUES(?,1,?,?,?)")
        .bind(&session.token_hash)
        .bind(&session.csrf_hash)
        .bind(session.created_at)
        .bind(session.expires_at)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    tx.commit().await.map_err(AppError::internal)?;
    authenticated_response(
        StatusCode::CREATED,
        username,
        &session.token,
        &session.csrf,
        use_secure_cookies(&headers, &state),
    )
}

async fn login_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: std::result::Result<Json<CredentialsInput>, JsonRejection>,
) -> ApiResult<Response> {
    require_requested_with(&headers)?;
    let Json(input) = input.map_err(auth_json_error)?;
    if !admin_initialized(&state.db).await? {
        return Err(AppError::conflict("尚未注册管理员账号"));
    }
    let (username, password) = validate_credentials(input)?;
    let row = sqlx::query("SELECT username,password_hash FROM administrators WHERE id=1")
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    let stored_username: String = row.get("username");
    let password_hash: String = row.get("password_hash");
    let password_valid = verify_password_bounded(&state, password, password_hash).await?;
    if !password_valid || !secrets_equal(&username, &stored_username) {
        return Err(AppError::unauthorized("用户名或密码错误"));
    }
    let (session, csrf) = create_session(&state).await?;
    authenticated_response(
        StatusCode::OK,
        stored_username,
        &session,
        &csrf,
        use_secure_cookies(&headers, &state),
    )
}

async fn logout_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: std::result::Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    require_requested_with(&headers)?;
    let _body = body.map_err(auth_body_error)?;
    let session = require_admin(&headers, &state, true).await?;
    sqlx::query("DELETE FROM admin_sessions WHERE token_hash=?")
        .bind(session.token_hash)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?;
    let secure = use_secure_cookies(&headers, &state);
    let mut response = Json(serde_json::json!({"ok": true})).into_response();
    append_cookie(
        response.headers_mut(),
        expired_cookie(SESSION_COOKIE, true, secure),
    )?;
    append_cookie(
        response.headers_mut(),
        expired_cookie(CSRF_COOKIE, false, secure),
    )?;
    no_store(&mut response);
    Ok(response)
}

async fn get_storage_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<StorageSettingsOutput>> {
    require_admin(&headers, &state, false).await?;
    Ok(Json(
        state
            .storage
            .public_settings()
            .await
            .map_err(AppError::internal)?,
    ))
}
async fn save_storage_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<StorageSettingsInput>,
) -> ApiResult<Json<StorageSettingsOutput>> {
    require_admin(&headers, &state, true).await?;
    let _mutation_guard = state
        .storage_mutation_gate
        .clone()
        .try_read_owned()
        .map_err(|_| AppError::conflict("存储迁移或旧存储清理正在运行，暂时不能修改配置"))?;
    Ok(Json(
        state
            .storage
            .save(input)
            .await
            .map_err(|error| AppError::bad(error.to_string()))?,
    ))
}
async fn test_storage_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<StorageSettingsInput>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, true).await?;
    let _guard = state.storage.gate.read().await;
    state
        .storage
        .test_candidate(&input)
        .await
        .map_err(|error| AppError::bad(format!("存储连接测试失败：{error:#}")))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

const STORAGE_MIGRATION_JOB_COLUMNS: &str = "id,status,source_backend,target_backend,total,completed,succeeded,failed,cancelled,cleanup_status,cleanup_completed,cleanup_failed,created_at,updated_at,activated_at,error";

async fn refresh_storage_migration_counts(db: &SqlitePool, job_id: &str) -> Result<()> {
    sqlx::query("UPDATE storage_migration_jobs SET completed=(SELECT COUNT(*) FROM storage_migration_items WHERE job_id=? AND status IN ('succeeded','failed','cancelled')),succeeded=(SELECT COUNT(*) FROM storage_migration_items WHERE job_id=? AND status='succeeded'),failed=(SELECT COUNT(*) FROM storage_migration_items WHERE job_id=? AND status='failed'),cancelled=(SELECT COUNT(*) FROM storage_migration_items WHERE job_id=? AND status='cancelled'),updated_at=? WHERE id=?")
        .bind(job_id)
        .bind(job_id)
        .bind(job_id)
        .bind(job_id)
        .bind(now())
        .bind(job_id)
        .execute(db)
        .await?;
    Ok(())
}

async fn migration_candidates(
    state: &AppState,
    job_id: &str,
) -> Result<(StorageCandidate, StorageCandidate)> {
    let row =
        sqlx::query("SELECT source_config,target_config FROM storage_migration_jobs WHERE id=?")
            .bind(job_id)
            .fetch_one(&state.db)
            .await?;
    let source_json = state
        .storage
        .decrypt(&row.get::<String, _>("source_config"))?;
    let target_json = state
        .storage
        .decrypt(&row.get::<String, _>("target_config"))?;
    Ok((
        serde_json::from_str(&source_json)?,
        serde_json::from_str(&target_json)?,
    ))
}

#[allow(clippy::too_many_arguments)]
async fn copy_storage_migration_item(
    state: &AppState,
    job_id: &str,
    item_id: &str,
    storage_key: &str,
    content_type: &str,
    expected_size: i64,
    source: &Arc<dyn BlobStore>,
    target: &Arc<dyn BlobStore>,
    token: &CancellationToken,
) -> Result<()> {
    let changed = sqlx::query("UPDATE storage_migration_items SET status='processing',error=NULL WHERE id=? AND job_id=? AND status='queued'")
        .bind(item_id)
        .bind(job_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if changed != 1 {
        return Ok(());
    }
    if token.is_cancelled() {
        sqlx::query("UPDATE storage_migration_items SET status='cancelled',error='管理员安全中断任务' WHERE id=?")
            .bind(item_id)
            .execute(&state.db)
            .await?;
        return Ok(());
    }
    let data = timeout(STORAGE_IO_TIMEOUT, source.get(storage_key))
        .await
        .context("读取源存储超时")??;
    if data.len() as i64 != expected_size {
        bail!("源对象大小与数据库记录不一致");
    }
    let digest = hex_digest(&Sha256::digest(&data));
    if token.is_cancelled() {
        sqlx::query("UPDATE storage_migration_items SET status='cancelled',error='管理员安全中断任务' WHERE id=?")
            .bind(item_id)
            .execute(&state.db)
            .await?;
        return Ok(());
    }
    if let Ok(Ok(existing)) = timeout(STORAGE_IO_TIMEOUT, target.get(storage_key)).await {
        if existing.len() as i64 == expected_size
            && hex_digest(&Sha256::digest(&existing)) == digest
        {
            sqlx::query("UPDATE storage_migration_items SET status='succeeded',sha256=?,error=NULL WHERE id=?")
                .bind(digest)
                .bind(item_id)
                .execute(&state.db)
                .await?;
            return Ok(());
        }
        target
            .delete(storage_key)
            .await
            .context("移除目标存储中的冲突对象失败")?;
    }
    target
        .put_atomic(storage_key, content_type, data)
        .await
        .context("写入目标存储失败")?;
    let verified = timeout(STORAGE_IO_TIMEOUT, target.get(storage_key))
        .await
        .context("读回目标对象超时")??;
    if verified.len() as i64 != expected_size || hex_digest(&Sha256::digest(&verified)) != digest {
        bail!("目标对象读回校验失败");
    }
    sqlx::query(
        "UPDATE storage_migration_items SET status='succeeded',sha256=?,error=NULL WHERE id=?",
    )
    .bind(digest)
    .bind(item_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn run_storage_migration(
    state: AppState,
    job_id: String,
    source_candidate: StorageCandidate,
    target_candidate: StorageCandidate,
    token: CancellationToken,
    _mutation_guard: tokio::sync::OwnedRwLockWriteGuard<()>,
) -> Result<()> {
    let changed = sqlx::query("UPDATE storage_migration_jobs SET status='running',updated_at=?,error=NULL WHERE id=? AND status='queued'")
        .bind(now())
        .bind(&job_id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if changed != 1 {
        bail!("迁移任务不在可启动状态");
    }
    let source = StorageService::build_store(&source_candidate).await?;
    let target = StorageService::build_store(&target_candidate).await?;
    target.healthcheck().await.context("目标存储连接测试失败")?;
    let rows = sqlx::query("SELECT id,storage_key,content_type,byte_size FROM storage_migration_items WHERE job_id=? AND status='queued' ORDER BY id")
        .bind(&job_id)
        .fetch_all(&state.db)
        .await?;
    let items = rows
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("id"),
                row.get::<String, _>("storage_key"),
                row.get::<String, _>("content_type"),
                row.get::<i64, _>("byte_size"),
            )
        })
        .collect::<Vec<_>>();
    let workers = state.workers.clamp(1, 8);
    stream::iter(items)
        .for_each_concurrent(workers, |(item_id, storage_key, content_type, byte_size)| {
            let state = state.clone();
            let job_id = job_id.clone();
            let source = source.clone();
            let target = target.clone();
            let token = token.clone();
            async move {
                if token.is_cancelled() {
                    let _ = sqlx::query("UPDATE storage_migration_items SET status='cancelled',error='管理员安全中断任务' WHERE id=? AND status='queued'")
                        .bind(&item_id)
                        .execute(&state.db)
                        .await;
                } else if let Err(error) = copy_storage_migration_item(
                    &state,
                    &job_id,
                    &item_id,
                    &storage_key,
                    &content_type,
                    byte_size,
                    &source,
                    &target,
                    &token,
                )
                .await
                {
                    warn!(job_id, item_id, "storage migration item failed: {error:#}");
                    let _ = sqlx::query("UPDATE storage_migration_items SET status='failed',error=? WHERE id=? AND status='processing'")
                        .bind(format!("{error:#}"))
                        .bind(&item_id)
                        .execute(&state.db)
                        .await;
                }
                let _ = refresh_storage_migration_counts(&state.db, &job_id).await;
            }
        })
        .await;
    if token.is_cancelled() {
        sqlx::query("UPDATE storage_migration_items SET status='cancelled',error=COALESCE(error,'管理员安全中断任务') WHERE job_id=? AND status IN ('queued','processing')")
            .bind(&job_id)
            .execute(&state.db)
            .await?;
    } else {
        sqlx::query("UPDATE storage_migration_items SET status='failed',error=COALESCE(error,'迁移线程未生成终态') WHERE job_id=? AND status IN ('queued','processing')")
            .bind(&job_id)
            .execute(&state.db)
            .await?;
    }
    refresh_storage_migration_counts(&state.db, &job_id).await?;
    let row = sqlx::query(
        "SELECT total,succeeded,failed,cancelled FROM storage_migration_jobs WHERE id=?",
    )
    .bind(&job_id)
    .fetch_one(&state.db)
    .await?;
    let total: i64 = row.get("total");
    let succeeded: i64 = row.get("succeeded");
    let failed: i64 = row.get("failed");
    let cancelled: i64 = row.get("cancelled");
    if token.is_cancelled() || cancelled > 0 {
        sqlx::query("UPDATE storage_migration_jobs SET status='cancelled',updated_at=?,error='管理员安全中断任务' WHERE id=?")
            .bind(now())
            .bind(&job_id)
            .execute(&state.db)
            .await?;
    } else if failed > 0 || succeeded != total {
        sqlx::query("UPDATE storage_migration_jobs SET status='failed',updated_at=?,error='部分对象迁移失败，可修复存储连接后继续任务' WHERE id=?")
            .bind(now())
            .bind(&job_id)
            .execute(&state.db)
            .await?;
    } else {
        state
            .storage
            .activate_migration_candidate(&job_id, &target_candidate)
            .await?;
    }
    Ok(())
}

fn spawn_storage_migration(
    state: AppState,
    job_id: String,
    source: StorageCandidate,
    target: StorageCandidate,
    guard: tokio::sync::OwnedRwLockWriteGuard<()>,
) {
    let token = CancellationToken::new();
    state.storage_tasks.insert(job_id.clone(), token.clone());
    tokio::spawn(async move {
        let inner_state = state.clone();
        let inner_job_id = job_id.clone();
        let outcome = tokio::spawn(async move {
            run_storage_migration(inner_state, inner_job_id, source, target, token, guard).await
        })
        .await;
        let failure = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(error) => Some(format!("迁移执行器异常退出：{error}")),
        };
        if let Some(reason) = failure {
            error!(job_id, "storage migration failed: {reason}");
            let _ = sqlx::query("UPDATE storage_migration_jobs SET status='failed',updated_at=?,error=? WHERE id=? AND status IN ('queued','running')")
                .bind(now())
                .bind(reason)
                .bind(&job_id)
                .execute(&state.db)
                .await;
            let _ = refresh_storage_migration_counts(&state.db, &job_id).await;
        }
        state.storage_tasks.remove(&job_id);
    });
}

async fn list_storage_migrations(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<StorageMigrationJob>>> {
    require_admin(&headers, &state, false).await?;
    let sql = format!(
        "SELECT {STORAGE_MIGRATION_JOB_COLUMNS} FROM storage_migration_jobs ORDER BY created_at DESC LIMIT 50"
    );
    let rows = sqlx::query(&sql)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(rows.iter().map(migration_job_from).collect()))
}

async fn start_storage_migration(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<StorageSettingsInput>,
) -> ApiResult<(StatusCode, Json<StorageMigrationJob>)> {
    require_admin(&headers, &state, true).await?;
    let guard = state
        .storage_mutation_gate
        .clone()
        .try_write_owned()
        .map_err(|_| AppError::conflict("仍有上传、转换、删除或存储任务正在运行"))?;
    let unresolved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM storage_migration_jobs WHERE status IN ('queued','running','failed','cancelled','interrupted') OR (status='completed' AND cleanup_status NOT IN ('cleaned','retained')))")
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    if unresolved {
        return Err(AppError::conflict(
            "已有未收尾的存储迁移；请继续任务并选择清理或保留旧存储",
        ));
    }
    let active_conversion: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversion_jobs WHERE status IN ('queued','running'))",
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::internal)?;
    let pending_objects: i64 = sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM pending_blobs) + (SELECT COUNT(*) FROM photo_deletion_outbox) + (SELECT COUNT(*) FROM source_deletion_outbox)")
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    if active_conversion || pending_objects > 0 {
        return Err(AppError::conflict(
            "请先结束图片转换并等待对象清理队列完成，再开始迁移",
        ));
    }
    let source = state
        .storage
        .current_candidate()
        .await
        .map_err(AppError::internal)?;
    let target = state
        .storage
        .candidate_from_input(&input)
        .await
        .map_err(|error| AppError::bad(error.to_string()))?;
    if source.location_key() == target.location_key() {
        return Err(AppError::bad("源存储和目标存储位置相同，无需迁移"));
    }
    let target_store = StorageService::build_store(&target)
        .await
        .map_err(AppError::internal)?;
    target_store
        .healthcheck()
        .await
        .map_err(|error| AppError::bad(format!("目标存储连接测试失败：{error:#}")))?;
    let photos =
        sqlx::query("SELECT id,storage_key,content_type,byte_size FROM photos ORDER BY id")
            .fetch_all(&state.db)
            .await
            .map_err(AppError::internal)?;
    if photos.is_empty() {
        return Err(AppError::bad("当前没有图片，请直接保存新的存储配置"));
    }
    let timestamp = now();
    let job_id = Uuid::new_v4().to_string();
    let source_config = state
        .storage
        .encrypt(&serde_json::to_string(&source).map_err(AppError::internal)?)
        .map_err(AppError::internal)?;
    let target_config = state
        .storage
        .encrypt(&serde_json::to_string(&target).map_err(AppError::internal)?)
        .map_err(AppError::internal)?;
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    sqlx::query("INSERT INTO storage_migration_jobs(id,status,source_backend,target_backend,total,source_config,target_config,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(&job_id)
        .bind("queued")
        .bind(&source.backend)
        .bind(&target.backend)
        .bind(photos.len() as i64)
        .bind(source_config)
        .bind(target_config)
        .bind(timestamp)
        .bind(timestamp)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    for photo in photos {
        sqlx::query("INSERT INTO storage_migration_items(id,job_id,photo_id,storage_key,content_type,byte_size,status) VALUES(?,?,?,?,?,?,'queued')")
            .bind(Uuid::new_v4().to_string())
            .bind(&job_id)
            .bind(photo.get::<String, _>("id"))
            .bind(photo.get::<String, _>("storage_key"))
            .bind(photo.get::<String, _>("content_type"))
            .bind(photo.get::<i64, _>("byte_size"))
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
    }
    tx.commit().await.map_err(AppError::internal)?;
    let sql =
        format!("SELECT {STORAGE_MIGRATION_JOB_COLUMNS} FROM storage_migration_jobs WHERE id=?");
    let row = sqlx::query(&sql)
        .bind(&job_id)
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    let job = migration_job_from(&row);
    spawn_storage_migration(state, job_id, source, target, guard);
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn resume_storage_migration(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    require_admin(&headers, &state, true).await?;
    let guard = state
        .storage_mutation_gate
        .clone()
        .try_write_owned()
        .map_err(|_| AppError::conflict("仍有上传、转换、删除或存储任务正在运行"))?;
    let status: String = sqlx::query_scalar("SELECT status FROM storage_migration_jobs WHERE id=?")
        .bind(&job_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::bad("迁移任务不存在"))?;
    if !matches!(status.as_str(), "failed" | "cancelled" | "interrupted") {
        return Err(AppError::bad("此任务当前不能继续"));
    }
    let (source, target) = migration_candidates(&state, &job_id)
        .await
        .map_err(AppError::internal)?;
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    sqlx::query("UPDATE storage_migration_items SET status='queued',error=NULL WHERE job_id=? AND status IN ('failed','cancelled','processing')")
        .bind(&job_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    sqlx::query("UPDATE storage_migration_jobs SET status='queued',completed=(SELECT COUNT(*) FROM storage_migration_items WHERE job_id=? AND status='succeeded'),succeeded=(SELECT COUNT(*) FROM storage_migration_items WHERE job_id=? AND status='succeeded'),failed=0,cancelled=0,updated_at=?,error=NULL WHERE id=?")
        .bind(&job_id)
        .bind(&job_id)
        .bind(now())
        .bind(&job_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    tx.commit().await.map_err(AppError::internal)?;
    spawn_storage_migration(state, job_id, source, target, guard);
    Ok(StatusCode::ACCEPTED)
}

async fn cancel_storage_task(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    require_admin(&headers, &state, true).await?;
    let token = state
        .storage_tasks
        .get(&job_id)
        .ok_or_else(|| AppError::bad("任务不在运行中，无法中断"))?;
    token.cancel();
    Ok(StatusCode::ACCEPTED)
}

async fn run_storage_cleanup(
    state: AppState,
    job_id: String,
    source_candidate: StorageCandidate,
    target_candidate: StorageCandidate,
    token: CancellationToken,
    _mutation_guard: tokio::sync::OwnedRwLockWriteGuard<()>,
) -> Result<()> {
    let source = StorageService::build_store(&source_candidate).await?;
    let target = StorageService::build_store(&target_candidate).await?;
    let rows = sqlx::query("SELECT id,storage_key,sha256 FROM storage_migration_items WHERE job_id=? AND status='succeeded' AND source_deleted_at IS NULL ORDER BY id")
        .bind(&job_id)
        .fetch_all(&state.db)
        .await?;
    let items = rows
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("id"),
                row.get::<String, _>("storage_key"),
                row.get::<String, _>("sha256"),
            )
        })
        .collect::<Vec<_>>();
    stream::iter(items)
        .for_each_concurrent(state.workers.clamp(1, 8), |(item_id, storage_key, digest)| {
            let state = state.clone();
            let job_id = job_id.clone();
            let source = source.clone();
            let target = target.clone();
            let token = token.clone();
            async move {
                if token.is_cancelled() {
                    return;
                }
                let result = async {
                    let target_data = timeout(STORAGE_IO_TIMEOUT, target.get(&storage_key))
                        .await
                        .context("清理前读回当前存储对象超时")??;
                    if hex_digest(&Sha256::digest(&target_data)) != digest {
                        bail!("当前存储对象校验失败，拒绝删除旧对象");
                    }
                    if token.is_cancelled() {
                        bail!("管理员安全中断任务");
                    }
                    source
                        .delete(&storage_key)
                        .await
                        .context("删除旧存储对象失败")?;
                    Ok::<_, anyhow::Error>(())
                }
                .await;
                match result {
                    Ok(()) => {
                        let _ = sqlx::query("UPDATE storage_migration_items SET source_deleted_at=?,cleanup_error=NULL WHERE id=?")
                            .bind(now())
                            .bind(&item_id)
                            .execute(&state.db)
                            .await;
                    }
                    Err(_error) if token.is_cancelled() => {}
                    Err(error) => {
                        warn!(job_id, item_id, "old storage cleanup failed: {error:#}");
                        let _ = sqlx::query("UPDATE storage_migration_items SET cleanup_error=? WHERE id=? AND source_deleted_at IS NULL")
                            .bind(format!("{error:#}"))
                            .bind(&item_id)
                            .execute(&state.db)
                            .await;
                    }
                }
            }
        })
        .await;
    let cleanup_completed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM storage_migration_items WHERE job_id=? AND source_deleted_at IS NOT NULL")
        .bind(&job_id)
        .fetch_one(&state.db)
        .await?;
    let cleanup_failed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM storage_migration_items WHERE job_id=? AND source_deleted_at IS NULL AND cleanup_error IS NOT NULL")
        .bind(&job_id)
        .fetch_one(&state.db)
        .await?;
    let total: i64 = sqlx::query_scalar("SELECT total FROM storage_migration_jobs WHERE id=?")
        .bind(&job_id)
        .fetch_one(&state.db)
        .await?;
    let (status, error) = if token.is_cancelled() {
        ("interrupted", Some("管理员安全中断旧存储清理"))
    } else if cleanup_completed == total {
        ("cleaned", None)
    } else {
        ("failed", Some("部分旧存储对象清理失败，可检查连接后重试"))
    };
    sqlx::query("UPDATE storage_migration_jobs SET cleanup_status=?,cleanup_completed=?,cleanup_failed=?,updated_at=?,error=? WHERE id=?")
        .bind(status)
        .bind(cleanup_completed)
        .bind(cleanup_failed)
        .bind(now())
        .bind(error)
        .bind(&job_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

fn spawn_storage_cleanup(
    state: AppState,
    job_id: String,
    source: StorageCandidate,
    target: StorageCandidate,
    guard: tokio::sync::OwnedRwLockWriteGuard<()>,
) {
    let token = CancellationToken::new();
    state.storage_tasks.insert(job_id.clone(), token.clone());
    tokio::spawn(async move {
        let inner_state = state.clone();
        let inner_job_id = job_id.clone();
        let outcome = tokio::spawn(async move {
            run_storage_cleanup(inner_state, inner_job_id, source, target, token, guard).await
        })
        .await;
        let failure = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(error) => Some(format!("旧存储清理执行器异常退出：{error}")),
        };
        if let Some(reason) = failure {
            error!(job_id, "old storage cleanup failed: {reason}");
            let _ = sqlx::query("UPDATE storage_migration_jobs SET cleanup_status='failed',updated_at=?,error=? WHERE id=? AND cleanup_status='cleaning'")
                .bind(now())
                .bind(reason)
                .bind(&job_id)
                .execute(&state.db)
                .await;
        }
        state.storage_tasks.remove(&job_id);
    });
}

async fn cleanup_old_storage(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    require_admin(&headers, &state, true).await?;
    let guard = state
        .storage_mutation_gate
        .clone()
        .try_write_owned()
        .map_err(|_| AppError::conflict("仍有上传、转换、删除或存储任务正在运行"))?;
    let row = sqlx::query("SELECT status,cleanup_status FROM storage_migration_jobs WHERE id=?")
        .bind(&job_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::bad("迁移任务不存在"))?;
    let status: String = row.get("status");
    let cleanup_status: String = row.get("cleanup_status");
    if status != "completed"
        || !matches!(
            cleanup_status.as_str(),
            "pending" | "failed" | "interrupted"
        )
    {
        return Err(AppError::bad("此任务当前不能清理旧存储"));
    }
    let (source, target_snapshot) = migration_candidates(&state, &job_id)
        .await
        .map_err(AppError::internal)?;
    let active_target = state
        .storage
        .current_candidate()
        .await
        .map_err(AppError::internal)?;
    if active_target.location_key() != target_snapshot.location_key() {
        return Err(AppError::conflict(
            "当前存储位置已改变，无法确认迁移目标，拒绝清理旧存储",
        ));
    }
    if source.location_key() == active_target.location_key() {
        return Err(AppError::conflict("旧存储与当前存储相同，拒绝删除"));
    }
    sqlx::query("UPDATE storage_migration_jobs SET cleanup_status='cleaning',cleanup_failed=0,updated_at=?,error=NULL WHERE id=?")
        .bind(now())
        .bind(&job_id)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?;
    spawn_storage_cleanup(state, job_id, source, active_target, guard);
    Ok(StatusCode::ACCEPTED)
}

async fn retain_old_storage(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    require_admin(&headers, &state, true).await?;
    let _guard = state
        .storage_mutation_gate
        .clone()
        .try_write_owned()
        .map_err(|_| AppError::conflict("仍有存储任务正在运行"))?;
    let changed = sqlx::query("UPDATE storage_migration_jobs SET cleanup_status='retained',updated_at=?,error=NULL WHERE id=? AND status='completed' AND cleanup_status IN ('pending','failed','interrupted')")
        .bind(now())
        .bind(&job_id)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?
        .rows_affected();
    if changed != 1 {
        return Err(AppError::bad("此任务当前不能选择保留旧存储"));
    }
    Ok(StatusCode::NO_CONTENT)
}

const S3_CLEANUP_JOB_COLUMNS: &str = "id,status,phase,scanned_objects,protected_objects,total,completed,deleted,failed,skipped,bytes_found,bytes_deleted,worker_count,managed_prefix,created_at,updated_at,error";

async fn protected_storage_keys(db: &SqlitePool) -> Result<HashSet<String>> {
    let mut keys = HashSet::new();
    for query in [
        "SELECT storage_key FROM photos",
        "SELECT storage_key FROM photo_deletion_outbox",
    ] {
        keys.extend(sqlx::query_scalar::<_, String>(query).fetch_all(db).await?);
    }
    for key in sqlx::query_scalar::<_, String>("SELECT key FROM pending_blobs")
        .fetch_all(db)
        .await?
    {
        keys.insert(staging_key(&key));
        keys.insert(key);
    }
    for row in sqlx::query("SELECT source_key,target_key FROM source_deletion_outbox")
        .fetch_all(db)
        .await?
    {
        let source_key: String = row.get("source_key");
        let target_key: String = row.get("target_key");
        keys.insert(staging_key(&source_key));
        keys.insert(source_key);
        keys.insert(staging_key(&target_key));
        keys.insert(target_key);
    }
    Ok(keys)
}

fn s3_cleanup_candidate(
    object: &S3ManagedObject,
    protected: &HashSet<String>,
    cutoff: i64,
) -> bool {
    object.last_modified <= cutoff && !protected.contains(&object.logical_key)
}

async fn refresh_s3_cleanup_counts(db: &SqlitePool, job_id: &str) -> Result<()> {
    sqlx::query("UPDATE s3_cleanup_jobs SET completed=(SELECT COUNT(*) FROM s3_cleanup_items WHERE job_id=? AND status IN ('deleted','failed','protected')),deleted=(SELECT COUNT(*) FROM s3_cleanup_items WHERE job_id=? AND status='deleted'),failed=(SELECT COUNT(*) FROM s3_cleanup_items WHERE job_id=? AND status='failed'),skipped=(SELECT COUNT(*) FROM s3_cleanup_items WHERE job_id=? AND status='protected'),bytes_deleted=COALESCE((SELECT SUM(byte_size) FROM s3_cleanup_items WHERE job_id=? AND status='deleted'),0),updated_at=? WHERE id=?")
        .bind(job_id)
        .bind(job_id)
        .bind(job_id)
        .bind(job_id)
        .bind(job_id)
        .bind(now())
        .bind(job_id)
        .execute(db)
        .await?;
    Ok(())
}

async fn run_s3_cleanup_scan(
    state: AppState,
    job_id: String,
    candidate: StorageCandidate,
    token: CancellationToken,
    _mutation_guard: tokio::sync::OwnedRwLockWriteGuard<()>,
) -> Result<()> {
    let store = StorageService::build_s3_store(&candidate).await?;
    let protected = protected_storage_keys(&state.db).await?;
    let cutoff = now() - S3_ORPHAN_GRACE_SECONDS;
    sqlx::query("DELETE FROM s3_cleanup_items WHERE job_id=?")
        .bind(&job_id)
        .execute(&state.db)
        .await?;
    let mut continuation_token: Option<String> = None;
    let mut scanned_objects = 0_i64;
    let mut protected_objects = 0_i64;
    let mut total = 0_i64;
    let mut bytes_found = 0_i64;
    loop {
        if token.is_cancelled() {
            sqlx::query("UPDATE s3_cleanup_jobs SET status='cancelled',updated_at=?,error='管理员安全中断 S3 扫描' WHERE id=?")
                .bind(now())
                .bind(&job_id)
                .execute(&state.db)
                .await?;
            return Ok(());
        }
        let (objects, next) = store
            .list_managed_objects_page(continuation_token.as_deref())
            .await?;
        if token.is_cancelled() {
            sqlx::query("UPDATE s3_cleanup_jobs SET status='cancelled',updated_at=?,error='管理员安全中断 S3 扫描' WHERE id=?")
                .bind(now())
                .bind(&job_id)
                .execute(&state.db)
                .await?;
            return Ok(());
        }
        let page_count = objects.len() as i64;
        let candidates = objects
            .into_iter()
            .filter(|object| s3_cleanup_candidate(object, &protected, cutoff))
            .collect::<Vec<_>>();
        let page_bytes = candidates
            .iter()
            .map(|object| object.byte_size)
            .sum::<i64>();
        let mut tx = state.db.begin().await?;
        for object in &candidates {
            sqlx::query("INSERT INTO s3_cleanup_items(id,job_id,object_key,logical_key,byte_size,last_modified,status) VALUES(?,?,?,?,?,?,'queued')")
                .bind(Uuid::new_v4().to_string())
                .bind(&job_id)
                .bind(&object.physical_key)
                .bind(&object.logical_key)
                .bind(object.byte_size)
                .bind(object.last_modified)
                .execute(&mut *tx)
                .await?;
        }
        scanned_objects += page_count;
        protected_objects += page_count - candidates.len() as i64;
        total += candidates.len() as i64;
        bytes_found += page_bytes;
        sqlx::query("UPDATE s3_cleanup_jobs SET scanned_objects=?,protected_objects=?,total=?,bytes_found=?,updated_at=? WHERE id=? AND status='running'")
            .bind(scanned_objects)
            .bind(protected_objects)
            .bind(total)
            .bind(bytes_found)
            .bind(now())
            .bind(&job_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        continuation_token = next;
        if continuation_token.is_none() {
            break;
        }
    }
    sqlx::query("UPDATE s3_cleanup_jobs SET status='ready',phase='ready',scanned_objects=?,protected_objects=?,total=?,completed=0,deleted=0,failed=0,skipped=0,bytes_found=?,bytes_deleted=0,updated_at=?,error=NULL WHERE id=?")
        .bind(scanned_objects)
        .bind(protected_objects)
        .bind(total)
        .bind(bytes_found)
        .bind(now())
        .bind(&job_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn run_s3_cleanup_delete(
    state: AppState,
    job_id: String,
    candidate: StorageCandidate,
    token: CancellationToken,
    _mutation_guard: tokio::sync::OwnedRwLockWriteGuard<()>,
) -> Result<()> {
    let store = StorageService::build_s3_store(&candidate).await?;
    let protected = Arc::new(protected_storage_keys(&state.db).await?);
    let rows = sqlx::query("SELECT id,object_key,logical_key FROM s3_cleanup_items WHERE job_id=? AND status='queued' ORDER BY id")
        .bind(&job_id)
        .fetch_all(&state.db)
        .await?;
    let items = rows
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("id"),
                row.get::<String, _>("object_key"),
                row.get::<String, _>("logical_key"),
            )
        })
        .collect::<Vec<_>>();
    stream::iter(items)
        .for_each_concurrent(
            DEFAULT_S3_CLEANUP_CONCURRENCY,
            |(item_id, object_key, logical_key)| {
                let state = state.clone();
                let store = store.clone();
                let token = token.clone();
                let protected = protected.clone();
                let job_id = job_id.clone();
                async move {
                    if token.is_cancelled() {
                        return;
                    }
                    let changed = sqlx::query("UPDATE s3_cleanup_items SET status='deleting',error=NULL WHERE id=? AND job_id=? AND status='queued'")
                        .bind(&item_id)
                        .bind(&job_id)
                        .execute(&state.db)
                        .await
                        .map(|result| result.rows_affected())
                        .unwrap_or_default();
                    if changed != 1 {
                        return;
                    }
                    if protected.contains(&logical_key) {
                        let _ = sqlx::query("UPDATE s3_cleanup_items SET status='protected',error='删除前发现数据库引用，已跳过' WHERE id=?")
                            .bind(&item_id)
                            .execute(&state.db)
                            .await;
                    } else if token.is_cancelled() {
                        let _ = sqlx::query("UPDATE s3_cleanup_items SET status='queued' WHERE id=? AND status='deleting'")
                            .bind(&item_id)
                            .execute(&state.db)
                            .await;
                    } else {
                        match store.delete_physical_object(&object_key).await {
                            Ok(()) => {
                                let _ = sqlx::query("UPDATE s3_cleanup_items SET status='deleted',error=NULL WHERE id=?")
                                    .bind(&item_id)
                                    .execute(&state.db)
                                    .await;
                            }
                            Err(error) => {
                                warn!(job_id, object_key, "S3 orphan cleanup failed: {error:#}");
                                let _ = sqlx::query("UPDATE s3_cleanup_items SET status='failed',error=? WHERE id=?")
                                    .bind(format!("{error:#}"))
                                    .bind(&item_id)
                                    .execute(&state.db)
                                    .await;
                            }
                        }
                    }
                    let _ = refresh_s3_cleanup_counts(&state.db, &job_id).await;
                }
            },
        )
        .await;
    sqlx::query("UPDATE s3_cleanup_items SET status='queued',error=COALESCE(error,'任务中断，等待继续') WHERE job_id=? AND status='deleting'")
        .bind(&job_id)
        .execute(&state.db)
        .await?;
    refresh_s3_cleanup_counts(&state.db, &job_id).await?;
    let row = sqlx::query("SELECT total,completed,failed,skipped FROM s3_cleanup_jobs WHERE id=?")
        .bind(&job_id)
        .fetch_one(&state.db)
        .await?;
    let total: i64 = row.get("total");
    let completed: i64 = row.get("completed");
    let failed: i64 = row.get("failed");
    let skipped: i64 = row.get("skipped");
    let (status, error) = if token.is_cancelled() {
        ("cancelled", Some("管理员安全中断 S3 清理".to_string()))
    } else if failed > 0 || completed != total {
        (
            "failed",
            Some("部分 S3 旧对象删除失败，可检查连接后继续".to_string()),
        )
    } else if skipped > 0 {
        (
            "completed",
            Some(format!(
                "{skipped} 个对象在删除前发现数据库引用，已安全跳过"
            )),
        )
    } else {
        ("completed", None)
    };
    sqlx::query("UPDATE s3_cleanup_jobs SET status=?,updated_at=?,error=? WHERE id=?")
        .bind(status)
        .bind(now())
        .bind(error)
        .bind(&job_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

fn spawn_s3_cleanup_scan(
    state: AppState,
    job_id: String,
    candidate: StorageCandidate,
    guard: tokio::sync::OwnedRwLockWriteGuard<()>,
) {
    let token = CancellationToken::new();
    state.s3_cleanup_tasks.insert(job_id.clone(), token.clone());
    tokio::spawn(async move {
        let inner_state = state.clone();
        let inner_job_id = job_id.clone();
        let outcome = tokio::spawn(async move {
            run_s3_cleanup_scan(inner_state, inner_job_id, candidate, token, guard).await
        })
        .await;
        let failure = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(error) => Some(format!("S3 扫描执行器异常退出：{error}")),
        };
        if let Some(reason) = failure {
            error!(job_id, "S3 orphan scan failed: {reason}");
            let _ = sqlx::query("UPDATE s3_cleanup_jobs SET status='failed',updated_at=?,error=? WHERE id=? AND status='running'")
                .bind(now())
                .bind(reason)
                .bind(&job_id)
                .execute(&state.db)
                .await;
        }
        state.s3_cleanup_tasks.remove(&job_id);
    });
}

fn spawn_s3_cleanup_delete(
    state: AppState,
    job_id: String,
    candidate: StorageCandidate,
    guard: tokio::sync::OwnedRwLockWriteGuard<()>,
) {
    let token = CancellationToken::new();
    state.s3_cleanup_tasks.insert(job_id.clone(), token.clone());
    tokio::spawn(async move {
        let inner_state = state.clone();
        let inner_job_id = job_id.clone();
        let outcome = tokio::spawn(async move {
            run_s3_cleanup_delete(inner_state, inner_job_id, candidate, token, guard).await
        })
        .await;
        let failure = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(error) => Some(format!("S3 清理执行器异常退出：{error}")),
        };
        if let Some(reason) = failure {
            error!(job_id, "S3 orphan cleanup failed: {reason}");
            let _ = refresh_s3_cleanup_counts(&state.db, &job_id).await;
            let _ = sqlx::query("UPDATE s3_cleanup_jobs SET status='failed',updated_at=?,error=? WHERE id=? AND status='running'")
                .bind(now())
                .bind(reason)
                .bind(&job_id)
                .execute(&state.db)
                .await;
        }
        state.s3_cleanup_tasks.remove(&job_id);
    });
}

async fn latest_s3_cleanup(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Option<S3CleanupJob>>> {
    require_admin(&headers, &state, false).await?;
    let sql = format!(
        "SELECT {S3_CLEANUP_JOB_COLUMNS} FROM s3_cleanup_jobs ORDER BY created_at DESC LIMIT 1"
    );
    let row = sqlx::query(&sql)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(row.as_ref().map(s3_cleanup_job_from)))
}

async fn start_s3_cleanup_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<(StatusCode, Json<S3CleanupJob>)> {
    require_admin(&headers, &state, true).await?;
    let guard = state
        .storage_mutation_gate
        .clone()
        .try_write_owned()
        .map_err(|_| AppError::conflict("仍有上传、转换、删除或存储任务正在运行"))?;
    let candidate = state
        .storage
        .current_candidate()
        .await
        .map_err(AppError::internal)?;
    if candidate.backend != "s3" {
        return Err(AppError::bad("只有当前活动存储为 S3 时才能扫描旧空间"));
    }
    let timestamp = now();
    let job_id = Uuid::new_v4().to_string();
    let managed_prefix = if candidate.s3_prefix.is_empty() {
        "albums/".to_string()
    } else {
        format!("{}/albums/", candidate.s3_prefix)
    };
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    sqlx::query("UPDATE s3_cleanup_jobs SET status='cancelled',updated_at=?,error='已被新的扫描结果替代' WHERE status='ready'")
        .bind(timestamp)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    sqlx::query("INSERT INTO s3_cleanup_jobs(id,status,phase,worker_count,location_key,managed_prefix,created_at,updated_at) VALUES(?,'running','scanning',?,?,?,?,?)")
        .bind(&job_id)
        .bind(DEFAULT_S3_CLEANUP_CONCURRENCY as i64)
        .bind(candidate.location_key())
        .bind(managed_prefix)
        .bind(timestamp)
        .bind(timestamp)
        .execute(&mut *tx)
        .await
        .map_err(|error| AppError::conflict(format!("无法开始 S3 扫描：{error}")))?;
    tx.commit().await.map_err(AppError::internal)?;
    let sql = format!("SELECT {S3_CLEANUP_JOB_COLUMNS} FROM s3_cleanup_jobs WHERE id=?");
    let row = sqlx::query(&sql)
        .bind(&job_id)
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    let job = s3_cleanup_job_from(&row);
    spawn_s3_cleanup_scan(state, job_id, candidate, guard);
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn start_s3_cleanup_delete(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    require_admin(&headers, &state, true).await?;
    let guard = state
        .storage_mutation_gate
        .clone()
        .try_write_owned()
        .map_err(|_| AppError::conflict("仍有上传、转换、删除或存储任务正在运行"))?;
    let row = sqlx::query("SELECT status,phase,location_key FROM s3_cleanup_jobs WHERE id=?")
        .bind(&job_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::bad("S3 清理任务不存在"))?;
    if row.get::<String, _>("status") != "ready" || row.get::<String, _>("phase") != "ready" {
        return Err(AppError::bad("请先完成一次 S3 旧空间扫描"));
    }
    let candidate = state
        .storage
        .current_candidate()
        .await
        .map_err(AppError::internal)?;
    if candidate.backend != "s3" || candidate.location_key() != row.get::<String, _>("location_key")
    {
        return Err(AppError::conflict("S3 存储位置已改变，请重新扫描后再清理"));
    }
    sqlx::query("UPDATE s3_cleanup_jobs SET status='running',phase='deleting',updated_at=?,error=NULL WHERE id=?")
        .bind(now())
        .bind(&job_id)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?;
    spawn_s3_cleanup_delete(state, job_id, candidate, guard);
    Ok(StatusCode::ACCEPTED)
}

async fn cancel_s3_cleanup(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    require_admin(&headers, &state, true).await?;
    let token = state
        .s3_cleanup_tasks
        .get(&job_id)
        .ok_or_else(|| AppError::bad("S3 扫描或清理任务不在运行中"))?;
    token.cancel();
    Ok(StatusCode::ACCEPTED)
}

async fn resume_s3_cleanup(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    require_admin(&headers, &state, true).await?;
    let guard = state
        .storage_mutation_gate
        .clone()
        .try_write_owned()
        .map_err(|_| AppError::conflict("仍有上传、转换、删除或存储任务正在运行"))?;
    let row = sqlx::query("SELECT status,phase,location_key FROM s3_cleanup_jobs WHERE id=?")
        .bind(&job_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::bad("S3 清理任务不存在"))?;
    let status: String = row.get("status");
    let phase: String = row.get("phase");
    if !matches!(status.as_str(), "failed" | "cancelled" | "interrupted") {
        return Err(AppError::bad("此 S3 清理任务当前不能继续"));
    }
    let candidate = state
        .storage
        .current_candidate()
        .await
        .map_err(AppError::internal)?;
    if candidate.backend != "s3" || candidate.location_key() != row.get::<String, _>("location_key")
    {
        return Err(AppError::conflict("S3 存储位置已改变，请重新扫描"));
    }
    if phase == "deleting" {
        sqlx::query("UPDATE s3_cleanup_items SET status='queued',error=NULL WHERE job_id=? AND status IN ('failed','deleting')")
            .bind(&job_id)
            .execute(&state.db)
            .await
            .map_err(AppError::internal)?;
        sqlx::query(
            "UPDATE s3_cleanup_jobs SET status='running',updated_at=?,error=NULL WHERE id=?",
        )
        .bind(now())
        .bind(&job_id)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?;
        refresh_s3_cleanup_counts(&state.db, &job_id)
            .await
            .map_err(AppError::internal)?;
        spawn_s3_cleanup_delete(state, job_id, candidate, guard);
    } else {
        sqlx::query("DELETE FROM s3_cleanup_items WHERE job_id=?")
            .bind(&job_id)
            .execute(&state.db)
            .await
            .map_err(AppError::internal)?;
        sqlx::query("UPDATE s3_cleanup_jobs SET status='running',phase='scanning',scanned_objects=0,protected_objects=0,total=0,completed=0,deleted=0,failed=0,skipped=0,bytes_found=0,bytes_deleted=0,updated_at=?,error=NULL WHERE id=?")
            .bind(now())
            .bind(&job_id)
            .execute(&state.db)
            .await
            .map_err(AppError::internal)?;
        spawn_s3_cleanup_scan(state, job_id, candidate, guard);
    }
    Ok(StatusCode::ACCEPTED)
}

async fn read_site_settings(db: &SqlitePool) -> ApiResult<SiteSettings> {
    let rows = sqlx::query("SELECT key,value FROM app_settings WHERE key LIKE 'site_%'")
        .fetch_all(db)
        .await
        .map_err(AppError::internal)?;
    let values = rows
        .iter()
        .map(|row| (row.get::<String, _>("key"), row.get::<String, _>("value")))
        .collect::<HashMap<_, _>>();
    let defaults = SiteSettings::defaults();
    Ok(SiteSettings {
        title: values.get("site_title").cloned().unwrap_or(defaults.title),
        slogan: values
            .get("site_slogan")
            .cloned()
            .unwrap_or(defaults.slogan),
        author: values
            .get("site_author")
            .cloned()
            .unwrap_or(defaults.author),
        avatar_url: values
            .get("site_avatar_url")
            .cloned()
            .unwrap_or(defaults.avatar_url),
        theme: values.get("site_theme").cloned().unwrap_or(defaults.theme),
    })
}

async fn get_site_settings(State(state): State<AppState>) -> ApiResult<Json<SiteSettings>> {
    Ok(Json(read_site_settings(&state.db).await?))
}

async fn save_site_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SiteSettings>,
) -> ApiResult<Json<SiteSettings>> {
    require_admin(&headers, &state, true).await?;
    let settings = input.normalize()?;
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    for (key, value) in [
        ("site_title", settings.title.as_str()),
        ("site_slogan", settings.slogan.as_str()),
        ("site_author", settings.author.as_str()),
        ("site_avatar_url", settings.avatar_url.as_str()),
        ("site_theme", settings.theme.as_str()),
    ] {
        sqlx::query("INSERT INTO app_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
            .bind(key)
            .bind(value)
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
    }
    tx.commit().await.map_err(AppError::internal)?;
    Ok(Json(settings))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewAlbum {
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Default)]
enum PatchString {
    #[default]
    Missing,
    Present(Option<String>),
}

impl<'de> Deserialize<'de> for PatchString {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer).map(Self::Present)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AlbumPatch {
    #[serde(default)]
    name: PatchString,
    #[serde(default)]
    description: PatchString,
    #[serde(default)]
    display_created_date: PatchString,
    #[serde(default)]
    photo_date_start: PatchString,
    #[serde(default)]
    photo_date_end: PatchString,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AlbumOrderInput {
    album_ids: Vec<String>,
}

fn normalize_album_name(value: Option<String>) -> ApiResult<String> {
    let name = value.unwrap_or_default().trim().to_string();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::bad("相簿名不能为空且不得超过 100 个字符"));
    }
    Ok(name)
}

fn normalize_album_description(value: Option<String>) -> ApiResult<String> {
    let description = value.unwrap_or_default().trim().to_string();
    if description.chars().count() > 1000 {
        return Err(AppError::bad("相簿简介不得超过 1000 个字符"));
    }
    Ok(description)
}

fn is_valid_album_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes[..4].iter().all(u8::is_ascii_digit)
        || !bytes[5..7].iter().all(u8::is_ascii_digit)
        || !bytes[8..].iter().all(u8::is_ascii_digit)
    {
        return false;
    }

    let year = value[..4].parse::<u32>().unwrap_or_default();
    let month = value[5..7].parse::<u32>().unwrap_or_default();
    let day = value[8..].parse::<u32>().unwrap_or_default();
    if year == 0 || !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap_year = year % 400 == 0 || (year % 4 == 0 && year % 100 != 0);
    let days_in_month = match month {
        2 if leap_year => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= days_in_month
}

fn normalize_album_date(value: Option<String>, label: &str) -> ApiResult<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() != value.len() || !is_valid_album_date(trimmed) {
        return Err(AppError::bad(format!(
            "{label}必须是有效的 YYYY-MM-DD 日期"
        )));
    }
    Ok(Some(value))
}

fn validate_photo_date_range(start: &Option<String>, end: &Option<String>) -> ApiResult<()> {
    match (start, end) {
        (None, None) => Ok(()),
        (Some(start), Some(end)) if start <= end => Ok(()),
        (Some(_), Some(_)) => Err(AppError::bad("图片开始日期不得晚于结束日期")),
        _ => Err(AppError::bad(
            "图片开始日期和结束日期必须同时设置或同时清除",
        )),
    }
}

async fn list_albums(State(state): State<AppState>) -> ApiResult<Json<Vec<Album>>> {
    let rows = sqlx::query(&format!(
        "{} GROUP BY a.id ORDER BY a.position ASC,a.created_at DESC,a.id ASC",
        album_covers::ALBUM_SELECT
    ))
    .fetch_all(&state.db)
    .await
    .map_err(AppError::internal)?;
    Ok(Json(rows.iter().map(album_from).collect()))
}
async fn album_detail(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
) -> ApiResult<Json<AlbumDetail>> {
    let row = sqlx::query(&format!(
        "{} WHERE a.id=? GROUP BY a.id",
        album_covers::ALBUM_SELECT
    ))
    .bind(&album_id)
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::internal)?
    .ok_or_else(|| AppError {
        status: StatusCode::NOT_FOUND,
        message: "相簿不存在".into(),
        clear_auth_cookies: None,
    })?;
    let photos = sqlx::query("SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos WHERE album_id=? ORDER BY created_at DESC,id DESC")
        .bind(&album_id)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(AlbumDetail {
        album: album_from(&row),
        photos: photos.iter().map(photo_from).collect(),
    }))
}
async fn create_album(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<NewAlbum>,
) -> ApiResult<(StatusCode, Json<Album>)> {
    require_admin(&headers, &state, true).await?;
    let name = normalize_album_name(Some(input.name))?;
    let description = normalize_album_description(input.description)?;
    let position: i64 = sqlx::query_scalar("SELECT COALESCE(MIN(position),0)-1 FROM albums")
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    let album = Album {
        id: Uuid::new_v4().to_string(),
        name,
        description,
        created_at: now(),
        display_created_date: None,
        photo_date_start: None,
        photo_date_end: None,
        position,
        photo_count: 0,
        cover: album_covers::Cover {
            cover_source: "auto",
            cover_photo_id: None,
            cover_url: None,
        },
    };
    sqlx::query("INSERT INTO albums(id,name,description,created_at,position) VALUES(?,?,?,?,?)")
        .bind(&album.id)
        .bind(&album.name)
        .bind(&album.description)
        .bind(album.created_at)
        .bind(album.position)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?;
    Ok((StatusCode::CREATED, Json(album)))
}

async fn patch_album(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
    headers: HeaderMap,
    Json(input): Json<AlbumPatch>,
) -> ApiResult<Json<Album>> {
    require_admin(&headers, &state, true).await?;

    let current = sqlx::query("SELECT name,description,display_created_date,photo_date_start,photo_date_end FROM albums WHERE id=?")
        .bind(&album_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError {
            status: StatusCode::NOT_FOUND,
            message: "相簿不存在".into(),
            clear_auth_cookies: None,
        })?;

    let mut changed = false;
    let name = match input.name {
        PatchString::Missing => current.get::<String, _>("name"),
        PatchString::Present(value) => {
            changed = true;
            normalize_album_name(value)?
        }
    };
    let description = match input.description {
        PatchString::Missing => current.get::<String, _>("description"),
        PatchString::Present(value) => {
            changed = true;
            normalize_album_description(value)?
        }
    };
    let display_created_date = match input.display_created_date {
        PatchString::Missing => current.get::<Option<String>, _>("display_created_date"),
        PatchString::Present(value) => {
            changed = true;
            normalize_album_date(value, "展示创建日期")?
        }
    };
    let (photo_date_start, photo_date_end) = match (input.photo_date_start, input.photo_date_end) {
        (PatchString::Missing, PatchString::Missing) => (
            current.get::<Option<String>, _>("photo_date_start"),
            current.get::<Option<String>, _>("photo_date_end"),
        ),
        (PatchString::Present(start), PatchString::Present(end)) => {
            let start = normalize_album_date(start, "图片开始日期")?;
            let end = normalize_album_date(end, "图片结束日期")?;
            validate_photo_date_range(&start, &end)?;
            changed = true;
            (start, end)
        }
        _ => {
            return Err(AppError::bad(
                "图片开始日期和结束日期必须同时设置或同时清除",
            ));
        }
    };
    if !changed {
        return Err(AppError::bad("至少需要提供一个相簿字段"));
    }

    sqlx::query("UPDATE albums SET name=?,description=?,display_created_date=?,photo_date_start=?,photo_date_end=? WHERE id=?")
        .bind(name)
        .bind(description)
        .bind(display_created_date)
        .bind(photo_date_start)
        .bind(photo_date_end)
        .bind(&album_id)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?;

    let row = sqlx::query(&format!(
        "{} WHERE a.id=? GROUP BY a.id",
        album_covers::ALBUM_SELECT
    ))
    .bind(&album_id)
    .fetch_one(&state.db)
    .await
    .map_err(AppError::internal)?;
    Ok(Json(album_from(&row)))
}

async fn reorder_albums(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AlbumOrderInput>,
) -> ApiResult<Json<Vec<Album>>> {
    require_admin(&headers, &state, true).await?;
    let current_ids = sqlx::query_scalar::<_, String>("SELECT id FROM albums")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?;
    let requested = input.album_ids.iter().cloned().collect::<HashSet<_>>();
    let current = current_ids.iter().cloned().collect::<HashSet<_>>();
    if requested.len() != input.album_ids.len()
        || requested.len() != current.len()
        || requested != current
    {
        return Err(AppError::bad("相簿顺序必须完整包含当前所有相簿且不得重复"));
    }

    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    for (position, album_id) in input.album_ids.iter().enumerate() {
        sqlx::query("UPDATE albums SET position=? WHERE id=?")
            .bind(position as i64)
            .bind(album_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
    }
    tx.commit().await.map_err(AppError::internal)?;
    list_albums(State(state)).await
}
async fn album_photos(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
) -> ApiResult<Json<Vec<Photo>>> {
    let rows = sqlx::query("SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos WHERE album_id=? ORDER BY created_at DESC,id DESC").bind(album_id).fetch_all(&state.db).await.map_err(AppError::internal)?;
    Ok(Json(rows.iter().map(photo_from).collect()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AlbumExportQuery {
    album_ids: String,
}

struct AlbumExportSelection {
    name: String,
    photos: Vec<(String, String)>,
}

struct PreparedArchivePhoto {
    source_path: PathBuf,
    archive_name: String,
}

struct CleanupFileStream {
    inner: ReaderStream<tokio::fs::File>,
    cleanup_dir: Option<PathBuf>,
}

struct ExportTempGuard {
    cleanup_dir: Option<PathBuf>,
}

impl ExportTempGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            cleanup_dir: Some(path),
        }
    }

    fn take(&mut self) -> PathBuf {
        self.cleanup_dir
            .take()
            .expect("export temporary directory can only be transferred once")
    }
}

fn schedule_export_cleanup(path: PathBuf) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            if let Err(error) = tokio::fs::remove_dir_all(&path).await
                && error.kind() != ErrorKind::NotFound
            {
                warn!(path = %path.display(), "album export cleanup failed: {error:#}");
            }
        });
    } else if let Err(error) = std::fs::remove_dir_all(&path)
        && error.kind() != ErrorKind::NotFound
    {
        warn!(path = %path.display(), "album export cleanup failed: {error:#}");
    }
}

impl Drop for ExportTempGuard {
    fn drop(&mut self) {
        if let Some(path) = self.cleanup_dir.take() {
            schedule_export_cleanup(path);
        }
    }
}

impl Stream for CleanupFileStream {
    type Item = std::result::Result<Bytes, std::io::Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        context: &mut TaskContext<'_>,
    ) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(context)
    }
}

impl Drop for CleanupFileStream {
    fn drop(&mut self) {
        if let Some(path) = self.cleanup_dir.take() {
            schedule_export_cleanup(path);
        }
    }
}

fn sanitize_export_name(value: &str, fallback: &str) -> String {
    let candidate = value
        .split(['/', '\\'])
        .next_back()
        .unwrap_or(value)
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .take(180)
        .collect::<String>();
    let trimmed = candidate.trim_matches(|character| matches!(character, '.' | ' '));
    if trimmed.is_empty() || matches!(trimmed, "." | "..") {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn unique_export_name(desired: String, used: &mut HashSet<String>) -> String {
    let mut candidate = desired.clone();
    let mut suffix = 2;
    while !used.insert(candidate.to_ascii_lowercase()) {
        candidate = match desired.rsplit_once('.') {
            Some((stem, extension)) if !stem.is_empty() => {
                format!("{stem} ({suffix}).{extension}")
            }
            _ => format!("{desired} ({suffix})"),
        };
        suffix += 1;
    }
    candidate
}

fn write_album_zip(path: &Path, photos: &[PreparedArchivePhoto]) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut archive = ZipWriter::new(std::io::BufWriter::new(file));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6))
        .large_file(true)
        .unix_permissions(0o644);
    for photo in photos {
        archive.start_file(&photo.archive_name, options)?;
        let mut source = std::fs::File::open(&photo.source_path)?;
        std::io::copy(&mut source, &mut archive)?;
    }
    let mut file = archive.finish()?;
    file.flush()?;
    Ok(())
}

fn write_nested_export_zip(path: &Path, albums: &[(PathBuf, String)]) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut archive = ZipWriter::new(std::io::BufWriter::new(file));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true)
        .unix_permissions(0o644);
    for (source_path, archive_name) in albums {
        archive.start_file(archive_name, options)?;
        let mut source = std::fs::File::open(source_path)?;
        std::io::copy(&mut source, &mut archive)?;
    }
    let mut file = archive.finish()?;
    file.flush()?;
    Ok(())
}

async fn export_albums(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AlbumExportQuery>,
) -> ApiResult<Response> {
    require_admin(&headers, &state, false).await?;
    let album_ids = query
        .album_ids
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if album_ids.is_empty() {
        return Err(AppError::bad("请至少选择一个相簿"));
    }
    if album_ids.len() > 64 {
        return Err(AppError::bad("一次最多打包 64 个相簿"));
    }
    let unique_ids = album_ids.iter().cloned().collect::<HashSet<_>>();
    if unique_ids.len() != album_ids.len() {
        return Err(AppError::bad("相簿选择中存在重复项"));
    }

    let mut selections = Vec::with_capacity(album_ids.len());
    for album_id in &album_ids {
        let name = sqlx::query_scalar::<_, String>("SELECT name FROM albums WHERE id=?")
            .bind(album_id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::internal)?
            .ok_or_else(|| AppError {
                status: StatusCode::NOT_FOUND,
                message: format!("相簿不存在：{album_id}"),
                clear_auth_cookies: None,
            })?;
        let photo_rows = sqlx::query(
            "SELECT storage_key,original_name FROM photos WHERE album_id=? ORDER BY created_at DESC,id DESC",
        )
        .bind(album_id)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?;
        selections.push(AlbumExportSelection {
            name,
            photos: photo_rows
                .iter()
                .map(|row| (row.get("storage_key"), row.get("original_name")))
                .collect(),
        });
    }

    let export_permit = timeout(
        Duration::from_secs(5),
        state.export_slots.clone().acquire_owned(),
    )
    .await
    .map_err(|_| AppError::too_many("当前打包任务较多，请稍后重试"))?
    .map_err(|_| AppError::internal(anyhow!("album export queue closed")))?;
    let temporary_dir = std::env::temp_dir().join(format!("chronoframe-export-{}", Uuid::new_v4()));
    let mut temporary_guard = ExportTempGuard::new(temporary_dir.clone());
    let build_result: Result<(PathBuf, String)> = async {
        tokio::fs::create_dir(&temporary_dir).await?;
        let _storage_guard = state.storage.gate.read().await;
        let store = state.storage.store().await?;
        let mut album_archives = Vec::with_capacity(selections.len());
        let mut used_album_names = HashSet::new();

        for (album_index, album) in selections.iter().enumerate() {
            let source_dir = temporary_dir.join(format!("album-{album_index}-files"));
            tokio::fs::create_dir(&source_dir).await?;
            let mut prepared_photos = Vec::with_capacity(album.photos.len());
            let mut used_photo_names = HashSet::new();
            for (photo_index, (storage_key, original_name)) in album.photos.iter().enumerate() {
                let data = timeout(STORAGE_IO_TIMEOUT, store.get(storage_key))
                    .await
                    .with_context(|| format!("读取图片超时：{original_name}"))??;
                let source_path = source_dir.join(format!("{photo_index}.blob"));
                tokio::fs::write(&source_path, data).await?;
                let archive_name = unique_export_name(
                    sanitize_export_name(original_name, "photo"),
                    &mut used_photo_names,
                );
                prepared_photos.push(PreparedArchivePhoto {
                    source_path,
                    archive_name,
                });
            }

            let archive_path = temporary_dir.join(format!("album-{album_index}.zip"));
            tokio::task::spawn_blocking({
                let archive_path = archive_path.clone();
                move || write_album_zip(&archive_path, &prepared_photos)
            })
            .await
            .context("相簿打包线程异常")??;
            tokio::fs::remove_dir_all(&source_dir).await?;
            let archive_name = unique_export_name(
                format!("{}.zip", sanitize_export_name(&album.name, "album")),
                &mut used_album_names,
            );
            album_archives.push((archive_path, archive_name));
        }

        if album_archives.len() == 1 {
            let (path, filename) = album_archives
                .into_iter()
                .next()
                .expect("single album export has an archive");
            Ok((path, filename))
        } else {
            let path = temporary_dir.join("chronoframe-albums.zip");
            tokio::task::spawn_blocking({
                let path = path.clone();
                move || write_nested_export_zip(&path, &album_archives)
            })
            .await
            .context("多相簿打包线程异常")??;
            Ok((path, "chronoframe-albums.zip".into()))
        }
    }
    .await;
    drop(export_permit);

    let (archive_path, download_name) = match build_result {
        Ok(result) => result,
        Err(error) => {
            if let Err(cleanup_error) = tokio::fs::remove_dir_all(&temporary_dir).await
                && cleanup_error.kind() != ErrorKind::NotFound
            {
                warn!(path = %temporary_dir.display(), "failed export cleanup after error: {cleanup_error:#}");
            }
            return Err(AppError::internal(error));
        }
    };
    let archive_size = match tokio::fs::metadata(&archive_path).await {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            let _ = tokio::fs::remove_dir_all(&temporary_dir).await;
            return Err(AppError::internal(error));
        }
    };
    let archive = match tokio::fs::File::open(&archive_path).await {
        Ok(file) => file,
        Err(error) => {
            let _ = tokio::fs::remove_dir_all(&temporary_dir).await;
            return Err(AppError::internal(error));
        }
    };
    let stream = CleanupFileStream {
        inner: ReaderStream::new(archive),
        cleanup_dir: Some(temporary_guard.take()),
    };
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&archive_size.to_string()).map_err(AppError::internal)?,
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"chronoframe-albums.zip\"; filename*=UTF-8''{}",
            urlencoding::encode(&download_name)
        ))
        .map_err(AppError::internal)?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
    Ok(response)
}

async fn list_photos(State(state): State<AppState>) -> ApiResult<Json<Vec<Photo>>> {
    let rows = sqlx::query("SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos ORDER BY created_at DESC,id DESC")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(rows.iter().map(photo_from).collect()))
}

async fn photo_detail(
    State(state): State<AppState>,
    AxumPath(photo_id): AxumPath<String>,
) -> ApiResult<Json<Photo>> {
    let row = sqlx::query("SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos WHERE id=?")
        .bind(photo_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError {
            status: StatusCode::NOT_FOUND,
            message: "图片不存在".into(),
            clear_auth_cookies: None,
        })?;
    Ok(Json(photo_from(&row)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletePhotosInput {
    photo_ids: Vec<String>,
}

async fn ensure_photo_deletable(
    db: &SqlitePool,
    photo_id: &str,
    storage_key: &str,
) -> ApiResult<()> {
    let conversion_reference: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM conversion_items WHERE status IN ('queued','processing') AND (source_photo_id=? OR target_photo_id=?))")
        .bind(photo_id)
        .bind(photo_id)
        .fetch_one(db)
        .await
        .map_err(AppError::internal)?;
    let pending_source_cleanup: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM source_deletion_outbox WHERE source_photo_id=? OR target_key=?)")
        .bind(photo_id)
        .bind(storage_key)
        .fetch_one(db)
        .await
        .map_err(AppError::internal)?;
    let unresolved_migration: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM storage_migration_items item JOIN storage_migration_jobs job ON job.id=item.job_id WHERE item.photo_id=? AND (job.status!='completed' OR job.cleanup_status NOT IN ('cleaned','retained')))")
        .bind(photo_id)
        .fetch_one(db)
        .await
        .map_err(AppError::internal)?;
    if conversion_reference || pending_source_cleanup {
        return Err(AppError::conflict(
            "所选图片正被转换任务或旧图清理任务使用，请先结束相关任务",
        ));
    }
    if unresolved_migration {
        return Err(AppError::conflict(
            "所选图片属于尚未收尾的存储迁移，请先继续迁移并处理旧存储",
        ));
    }
    Ok(())
}

async fn delete_photo_records(
    state: &AppState,
    photo_ids: Vec<String>,
) -> ApiResult<serde_json::Value> {
    let unique = photo_ids.into_iter().collect::<HashSet<_>>();
    if unique.is_empty() {
        return Err(AppError::bad("请至少选择一张图片"));
    }
    if unique.len() > 500 {
        return Err(AppError::bad("一次最多删除 500 张图片"));
    }
    let _mutation_guard = state
        .storage_mutation_gate
        .clone()
        .try_read_owned()
        .map_err(|_| AppError::conflict("存储正在迁移或清理，请稍后再删除图片"))?;
    let _photo_graph_guard = state.photo_graph_lock.lock().await;
    let _storage_guard = state.storage.gate.read().await;
    let mut query = QueryBuilder::<Sqlite>::new("SELECT id,storage_key FROM photos WHERE id IN (");
    {
        let mut separated = query.separated(",");
        for id in &unique {
            separated.push_bind(id);
        }
    }
    query.push(")");
    let rows = query
        .build()
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?;
    if rows.len() != unique.len() {
        return Err(AppError::bad("部分图片已不存在，请刷新相簿后重试"));
    }
    for row in &rows {
        let photo_id: String = row.get("id");
        let storage_key: String = row.get("storage_key");
        ensure_photo_deletable(&state.db, &photo_id, &storage_key).await?;
    }
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    for row in &rows {
        let photo_id: String = row.get("id");
        let storage_key: String = row.get("storage_key");
        sqlx::query(
            "INSERT INTO photo_deletion_outbox(photo_id,storage_key,created_at) VALUES(?,?,?)",
        )
        .bind(&photo_id)
        .bind(&storage_key)
        .bind(now())
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
        sqlx::query("DELETE FROM photos WHERE id=?")
            .bind(&photo_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
    }
    tx.commit().await.map_err(AppError::internal)?;
    let drain = drain_photo_deletion_outbox(&state.storage, &state.db)
        .await
        .map_err(AppError::internal)?;
    Ok(serde_json::json!({
        "deleted": rows.len(),
        "objectsRemoved": drain.removed,
        "cleanupPending": drain.failures.len(),
        "failures": drain.failures
    }))
}

async fn delete_photo(
    State(state): State<AppState>,
    AxumPath(photo_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, true).await?;
    Ok(Json(delete_photo_records(&state, vec![photo_id]).await?))
}

async fn delete_photos(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<DeletePhotosInput>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, true).await?;
    Ok(Json(delete_photo_records(&state, input.photo_ids).await?))
}

async fn delete_album(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, true).await?;
    let _mutation_guard = state
        .storage_mutation_gate
        .clone()
        .try_read_owned()
        .map_err(|_| AppError::conflict("存储正在迁移或清理，请稍后再删除相簿"))?;
    let _photo_graph_guard = state.photo_graph_lock.lock().await;
    let _storage_guard = state.storage.gate.read().await;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM albums WHERE id=?)")
        .bind(&album_id)
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    if !exists {
        return Err(AppError {
            status: StatusCode::NOT_FOUND,
            message: "相簿不存在".into(),
            clear_auth_cookies: None,
        });
    }
    let rows = sqlx::query("SELECT id,storage_key FROM photos WHERE album_id=? ORDER BY id")
        .bind(&album_id)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?;
    for row in &rows {
        ensure_photo_deletable(
            &state.db,
            &row.get::<String, _>("id"),
            &row.get::<String, _>("storage_key"),
        )
        .await?;
    }
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    for row in &rows {
        sqlx::query(
            "INSERT INTO photo_deletion_outbox(photo_id,storage_key,created_at) VALUES(?,?,?)",
        )
        .bind(row.get::<String, _>("id"))
        .bind(row.get::<String, _>("storage_key"))
        .bind(now())
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    }
    let deleted = sqlx::query("DELETE FROM albums WHERE id=?")
        .bind(&album_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?
        .rows_affected();
    if deleted != 1 {
        return Err(AppError::conflict("相簿已由其他请求删除，请刷新页面"));
    }
    sqlx::query("WITH ranked AS (SELECT id,ROW_NUMBER() OVER (ORDER BY position ASC,created_at DESC,id ASC)-1 new_position FROM albums) UPDATE albums SET position=(SELECT new_position FROM ranked WHERE ranked.id=albums.id)")
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    tx.commit().await.map_err(AppError::internal)?;
    let drain = drain_photo_deletion_outbox(&state.storage, &state.db)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(serde_json::json!({
        "deleted": true,
        "photosDeleted": rows.len(),
        "objectsRemoved": drain.removed,
        "cleanupPending": drain.failures.len(),
        "failures": drain.failures
    })))
}

async fn upload_photos(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<Json<Vec<Photo>>> {
    require_admin(&headers, &state, true).await?;
    let _mutation_guard = state
        .storage_mutation_gate
        .clone()
        .try_read_owned()
        .map_err(|_| AppError::conflict("存储正在迁移或清理，请稍后再上传"))?;
    let _upload_permit = state
        .upload_slots
        .acquire()
        .await
        .map_err(|_| AppError::bad("上传队列已关闭"))?;
    let _storage_guard = state.storage.gate.read().await;
    let exists: Option<String> = sqlx::query_scalar("SELECT id FROM albums WHERE id=?")
        .bind(&album_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?;
    if exists.is_none() {
        return Err(AppError {
            status: StatusCode::NOT_FOUND,
            message: "相簿不存在；请先创建相簿".into(),
            clear_auth_cookies: None,
        });
    }
    // Validate the complete request before writing anything. The dashboard submits one file per
    // request so successful files remain visible even when a later file is invalid.
    let mut prepared: Vec<(Photo, Vec<u8>)> = vec![];
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad(e.to_string()))?
    {
        let filename = field.file_name().unwrap_or("image").to_string();
        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::bad(e.to_string()))?
            .to_vec();
        if bytes.is_empty() {
            continue;
        }
        let format = file_format(&filename)
            .ok_or_else(|| AppError::bad(format!("{filename} 仅支持 PNG、JPG/JPEG、WEBP")))?;
        let detected = image::guess_format(&bytes)
            .map_err(|_| AppError::bad(format!("{filename} 不是可识别的图片")))?;
        let detected_format = match detected {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::WebP => "webp",
            _ => return Err(AppError::bad(format!("{filename} 的内容格式不受支持"))),
        };
        if detected_format != format {
            return Err(AppError::bad(format!(
                "{filename} 的扩展名与实际图片格式不一致"
            )));
        }
        let (bytes, dimensions) = tokio::task::spawn_blocking(move || {
            let dimensions = image::load_from_memory(&bytes)
                .ok()
                .map(|decoded| (decoded.width() as i64, decoded.height() as i64));
            (bytes, dimensions)
        })
        .await
        .map_err(AppError::internal)?;
        let (width, height) =
            dimensions.ok_or_else(|| AppError::bad(format!("{filename} 不是有效图片")))?;
        let id = Uuid::new_v4().to_string();
        let key = format!("albums/{album_id}/original/{id}.{format}");
        let content_type = mime_for(&format).to_string();
        prepared.push((
            Photo {
                id,
                album_id: album_id.clone(),
                original_name: filename,
                storage_key: key,
                format,
                content_type,
                byte_size: bytes.len() as i64,
                width,
                height,
                created_at: now(),
            },
            bytes,
        ));
    }
    if prepared.is_empty() {
        return Err(AppError::bad("没有收到可上传的图片"));
    }
    let store = state.storage.store().await.map_err(AppError::internal)?;
    for (photo, bytes) in &mut prepared {
        register_pending_blob(&state.db, &photo.storage_key)
            .await
            .map_err(AppError::internal)?;
        // On an ambiguous remote error the ledger deliberately remains. The background janitor waits past the I/O deadline before deleting it.
        store
            .put_atomic(
                &photo.storage_key,
                &photo.content_type,
                std::mem::take(bytes),
            )
            .await
            .map_err(AppError::internal)?;
    }
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    for (photo, _) in &prepared {
        sqlx::query("INSERT INTO photos(id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at) VALUES(?,?,?,?,?,?,?,?,?,?)").bind(&photo.id).bind(&photo.album_id).bind(&photo.original_name).bind(&photo.storage_key).bind(&photo.format).bind(&photo.content_type).bind(photo.byte_size).bind(photo.width).bind(photo.height).bind(photo.created_at).execute(&mut *tx).await.map_err(AppError::internal)?;
        sqlx::query("DELETE FROM pending_blobs WHERE key=?")
            .bind(&photo.storage_key)
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
    }
    tx.commit().await.map_err(AppError::internal)?;
    let photos = prepared
        .into_iter()
        .map(|(photo, _)| photo)
        .collect::<Vec<_>>();
    spawn_thumbnail_generation(
        state.clone(),
        photos
            .iter()
            .map(|photo| (photo.id.clone(), photo.storage_key.clone()))
            .collect(),
    );
    Ok(Json(photos))
}
async fn photo_file(
    State(state): State<AppState>,
    AxumPath(photo_id): AxumPath<String>,
) -> ApiResult<Response> {
    let _storage_guard = state.storage.gate.read().await;
    let row = sqlx::query("SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos WHERE id=?").bind(photo_id).fetch_optional(&state.db).await.map_err(AppError::internal)?.ok_or_else(|| AppError { status: StatusCode::NOT_FOUND, message: "图片不存在".into(), clear_auth_cookies: None })?;
    let photo = photo_from(&row);
    let data = state
        .storage
        .store()
        .await
        .map_err(AppError::internal)?
        .get(&photo.storage_key)
        .await
        .map_err(AppError::internal)?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, photo.content_type),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable".to_string(),
            ),
        ],
        data,
    )
        .into_response())
}

fn derivative_bytes_are_valid(data: &[u8], derivative: ImageDerivative) -> bool {
    let expected = match derivative {
        ImageDerivative::Grid => ImageFormat::Png,
        ImageDerivative::Preview | ImageDerivative::High => ImageFormat::WebP,
    };
    image::guess_format(data).ok() == Some(expected)
}

async fn cached_valid_derivative(
    state: &AppState,
    photo_id: &str,
    derivative: ImageDerivative,
) -> Result<Option<Vec<u8>>> {
    match state
        .storage
        .cached_derivative(photo_id, derivative)
        .await?
    {
        Some(data) if derivative_bytes_are_valid(&data, derivative) => Ok(Some(data)),
        Some(_) => {
            state.storage.remove_cached_thumbnail(photo_id).await;
            Ok(None)
        }
        None => Ok(None),
    }
}

async fn ensure_image_derivative(
    state: &AppState,
    photo_id: &str,
    storage_key: &str,
    derivative: ImageDerivative,
) -> Result<Vec<u8>> {
    let _maintenance_guard = state.storage.thumbnail_maintenance_gate.read().await;
    if let Some(data) = cached_valid_derivative(state, photo_id, derivative).await? {
        return Ok(data);
    }
    let thumbnail_lock = state
        .storage
        .thumbnail_locks
        .entry(photo_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let _thumbnail_guard = thumbnail_lock.lock().await;
    if let Some(data) = cached_valid_derivative(state, photo_id, derivative).await? {
        return Ok(data);
    }
    let _permit = state
        .thumbnail_slots
        .acquire()
        .await
        .map_err(|_| anyhow!("缩略图工作池已关闭"))?;
    let data = {
        let _storage_guard = state.storage.gate.read().await;
        state.storage.store().await?.get(storage_key).await?
    };
    let encoded = tokio::task::spawn_blocking(move || encode_derivative(&data, derivative))
        .await
        .context("图片派生版本编码线程异常")??;
    let photo_still_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM photos WHERE id=?)")
            .bind(photo_id)
            .fetch_one(&state.db)
            .await?;
    if !photo_still_exists {
        bail!("图片已删除，取消写入派生版本");
    }
    state
        .storage
        .cache_derivative(photo_id, derivative, &encoded)
        .await?;
    Ok(encoded)
}

async fn ensure_all_image_derivatives(
    state: &AppState,
    photo_id: &str,
    storage_key: &str,
) -> Result<()> {
    let _maintenance_guard = state.storage.thumbnail_maintenance_gate.read().await;
    let derivatives = [
        ImageDerivative::Grid,
        ImageDerivative::Preview,
        ImageDerivative::High,
    ];
    let mut all_ready = true;
    for derivative in derivatives {
        if cached_valid_derivative(state, photo_id, derivative)
            .await?
            .is_none()
        {
            all_ready = false;
            break;
        }
    }
    if all_ready {
        return Ok(());
    }
    let thumbnail_lock = state
        .storage
        .thumbnail_locks
        .entry(photo_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let _thumbnail_guard = thumbnail_lock.lock().await;
    let mut all_ready = true;
    for derivative in derivatives {
        if cached_valid_derivative(state, photo_id, derivative)
            .await?
            .is_none()
        {
            all_ready = false;
            break;
        }
    }
    if all_ready {
        return Ok(());
    }
    let _permit = state
        .thumbnail_slots
        .acquire()
        .await
        .map_err(|_| anyhow!("图片派生版本工作池已关闭"))?;
    let data = {
        let _storage_guard = state.storage.gate.read().await;
        state.storage.store().await?.get(storage_key).await?
    };
    let encoded = tokio::task::spawn_blocking(move || encode_all_derivatives(&data))
        .await
        .context("图片三层派生版本编码线程异常")??;
    let photo_still_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM photos WHERE id=?)")
            .bind(photo_id)
            .fetch_one(&state.db)
            .await?;
    if !photo_still_exists {
        bail!("图片已删除，取消写入派生版本");
    }
    for (derivative, data) in encoded {
        state
            .storage
            .cache_derivative(photo_id, derivative, &data)
            .await?;
    }
    Ok(())
}

async fn generate_thumbnail_batch(state: AppState, photos: Vec<(String, String)>) {
    let total = photos.len();
    if total == 0 {
        return;
    }
    let outcomes = stream::iter(photos.into_iter().map(|(photo_id, storage_key)| {
        let state = state.clone();
        async move {
            let mut last_error = None;
            for attempt in 1..=3 {
                match ensure_all_image_derivatives(&state, &photo_id, &storage_key).await {
                    Ok(()) => return true,
                    Err(error) => {
                        last_error = Some(error);
                        if attempt < 3 {
                            tokio::time::sleep(Duration::from_secs(1_u64 << (attempt - 1))).await;
                        }
                    }
                }
            }
            if let Some(error) = last_error {
                warn!(
                    photo_id,
                    "automatic three-tier image generation deferred: {error:#}"
                );
            }
            false
        }
    }))
    .buffer_unordered(state.thumbnail_workers)
    .collect::<Vec<_>>()
    .await;
    let ready = outcomes.into_iter().filter(|success| *success).count();
    info!(
        total,
        ready,
        failed = total - ready,
        "automatic three-tier image batch finished"
    );
}

fn automatic_thumbnail_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get().saturating_mul(2))
        .unwrap_or(DEFAULT_THUMBNAIL_CONCURRENCY)
        .clamp(DEFAULT_THUMBNAIL_CONCURRENCY, 32)
}

fn spawn_thumbnail_generation(state: AppState, photos: Vec<(String, String)>) {
    tokio::spawn(generate_thumbnail_batch(state, photos));
}

fn spawn_thumbnail_backfill(state: AppState) {
    tokio::spawn(async move {
        let rows = match sqlx::query("SELECT id,storage_key FROM photos ORDER BY created_at,id")
            .fetch_all(&state.db)
            .await
        {
            Ok(rows) => rows,
            Err(error) => {
                warn!("thumbnail backfill inventory failed: {error:#}");
                return;
            }
        };
        let photos = rows
            .iter()
            .map(|row| (row.get("id"), row.get("storage_key")))
            .collect();
        generate_thumbnail_batch(state, photos).await;
    });
}

const THUMBNAIL_REBUILD_JOB_COLUMNS: &str = "id,status,phase,total,completed,succeeded,failed,skipped,cancelled,cache_files_removed,worker_count,created_at,updated_at,error";

async fn refresh_thumbnail_rebuild_counts(db: &SqlitePool, job_id: &str) -> Result<()> {
    sqlx::query("UPDATE thumbnail_rebuild_jobs SET completed=(SELECT COUNT(*) FROM thumbnail_rebuild_items WHERE job_id=? AND status IN ('succeeded','failed','skipped','cancelled')),succeeded=(SELECT COUNT(*) FROM thumbnail_rebuild_items WHERE job_id=? AND status='succeeded'),failed=(SELECT COUNT(*) FROM thumbnail_rebuild_items WHERE job_id=? AND status='failed'),skipped=(SELECT COUNT(*) FROM thumbnail_rebuild_items WHERE job_id=? AND status='skipped'),cancelled=(SELECT COUNT(*) FROM thumbnail_rebuild_items WHERE job_id=? AND status='cancelled'),updated_at=? WHERE id=?")
        .bind(job_id)
        .bind(job_id)
        .bind(job_id)
        .bind(job_id)
        .bind(job_id)
        .bind(now())
        .bind(job_id)
        .execute(db)
        .await?;
    Ok(())
}

async fn finish_thumbnail_rebuild_item(
    state: &AppState,
    job_id: &str,
    item_id: &str,
    status: &str,
    error: Option<String>,
) -> Result<()> {
    sqlx::query("UPDATE thumbnail_rebuild_items SET status=?,error=? WHERE id=? AND job_id=? AND status='processing'")
        .bind(status)
        .bind(error)
        .bind(item_id)
        .bind(job_id)
        .execute(&state.db)
        .await?;
    let (succeeded, failed, skipped) = match status {
        "succeeded" => (1, 0, 0),
        "failed" => (0, 1, 0),
        "skipped" => (0, 0, 1),
        _ => (0, 0, 0),
    };
    sqlx::query("UPDATE thumbnail_rebuild_jobs SET completed=completed+1,succeeded=succeeded+?,failed=failed+?,skipped=skipped+?,updated_at=? WHERE id=?")
        .bind(succeeded)
        .bind(failed)
        .bind(skipped)
        .bind(now())
        .bind(job_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn process_thumbnail_rebuild_item(
    state: AppState,
    job_id: String,
    item_id: String,
    photo_id: String,
    storage_key: String,
) -> Result<()> {
    let claimed = sqlx::query("UPDATE thumbnail_rebuild_items SET status='processing',error=NULL WHERE id=? AND job_id=? AND status='queued'")
        .bind(&item_id)
        .bind(&job_id)
        .execute(&state.db)
        .await?;
    if claimed.rows_affected() != 1 {
        return Ok(());
    }
    let current_storage_key: Option<String> =
        sqlx::query_scalar("SELECT storage_key FROM photos WHERE id=?")
            .bind(&photo_id)
            .fetch_optional(&state.db)
            .await?;
    if current_storage_key.as_deref() != Some(storage_key.as_str()) {
        return finish_thumbnail_rebuild_item(
            &state,
            &job_id,
            &item_id,
            "skipped",
            Some("图片已删除或存储对象已被替换".into()),
        )
        .await;
    }
    let mut last_error = None;
    for attempt in 1..=3 {
        match ensure_all_image_derivatives(&state, &photo_id, &storage_key).await {
            Ok(()) => {
                return finish_thumbnail_rebuild_item(&state, &job_id, &item_id, "succeeded", None)
                    .await;
            }
            Err(error) => {
                last_error = Some(format!("{error:#}"));
                if attempt < 3 {
                    tokio::time::sleep(Duration::from_secs(1_u64 << (attempt - 1))).await;
                }
            }
        }
    }
    finish_thumbnail_rebuild_item(&state, &job_id, &item_id, "failed", last_error).await
}

async fn run_thumbnail_rebuild(
    state: AppState,
    job_id: String,
    token: CancellationToken,
    _storage_guard: tokio::sync::OwnedRwLockReadGuard<()>,
) -> Result<()> {
    let phase: String = sqlx::query_scalar("SELECT phase FROM thumbnail_rebuild_jobs WHERE id=?")
        .bind(&job_id)
        .fetch_one(&state.db)
        .await?;
    let changed = sqlx::query("UPDATE thumbnail_rebuild_jobs SET status='running',updated_at=?,error=NULL WHERE id=? AND status='queued'")
        .bind(now())
        .bind(&job_id)
        .execute(&state.db)
        .await?;
    if changed.rows_affected() != 1 {
        bail!("缩略图重建任务状态已经改变");
    }

    if phase != "generating" {
        sqlx::query("UPDATE thumbnail_rebuild_jobs SET phase='clearing',updated_at=? WHERE id=?")
            .bind(now())
            .bind(&job_id)
            .execute(&state.db)
            .await?;
        let clear = state.storage.clear_thumbnail_cache();
        tokio::pin!(clear);
        let removed = tokio::select! {
            result = &mut clear => result?,
            _ = token.cancelled() => {
                sqlx::query("UPDATE thumbnail_rebuild_items SET status='cancelled',error='管理员安全中断任务' WHERE job_id=? AND status IN ('queued','processing')")
                    .bind(&job_id).execute(&state.db).await?;
                refresh_thumbnail_rebuild_counts(&state.db, &job_id).await?;
                sqlx::query("UPDATE thumbnail_rebuild_jobs SET status='cancelled',updated_at=?,error='管理员安全中断任务' WHERE id=?")
                    .bind(now()).bind(&job_id).execute(&state.db).await?;
                return Ok(());
            }
        };
        sqlx::query("UPDATE thumbnail_rebuild_jobs SET phase='generating',cache_files_removed=?,updated_at=? WHERE id=?")
            .bind(removed as i64)
            .bind(now())
            .bind(&job_id)
            .execute(&state.db)
            .await?;
    }

    let rows = sqlx::query("SELECT id,photo_id,storage_key FROM thumbnail_rebuild_items WHERE job_id=? AND status='queued' ORDER BY id")
        .bind(&job_id)
        .fetch_all(&state.db)
        .await?;
    let items = rows
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("id"),
                row.get::<String, _>("photo_id"),
                row.get::<String, _>("storage_key"),
            )
        })
        .collect::<Vec<_>>();
    let mut work = stream::iter(items.into_iter().map(|(item_id, photo_id, storage_key)| {
        process_thumbnail_rebuild_item(
            state.clone(),
            job_id.clone(),
            item_id,
            photo_id,
            storage_key,
        )
    }))
    .buffer_unordered(state.thumbnail_workers);
    let mut was_cancelled = false;
    loop {
        tokio::select! {
            _ = token.cancelled() => {
                was_cancelled = true;
                break;
            }
            result = work.next() => match result {
                Some(result) => result?,
                None => break,
            }
        }
    }
    drop(work);
    if was_cancelled {
        sqlx::query("UPDATE thumbnail_rebuild_items SET status='cancelled',error='管理员安全中断任务' WHERE job_id=? AND status IN ('queued','processing')")
            .bind(&job_id)
            .execute(&state.db)
            .await?;
    }
    refresh_thumbnail_rebuild_counts(&state.db, &job_id).await?;
    let row = sqlx::query("SELECT failed,cancelled FROM thumbnail_rebuild_jobs WHERE id=?")
        .bind(&job_id)
        .fetch_one(&state.db)
        .await?;
    let failed: i64 = row.get("failed");
    let cancelled: i64 = row.get("cancelled");
    let (status, error) = if was_cancelled || cancelled > 0 {
        ("cancelled", Some("管理员安全中断任务"))
    } else if failed > 0 {
        ("failed", Some("部分缩略图生成失败；可以继续任务重试失败项"))
    } else {
        ("completed", None)
    };
    sqlx::query("UPDATE thumbnail_rebuild_jobs SET status=?,updated_at=?,error=? WHERE id=?")
        .bind(status)
        .bind(now())
        .bind(error)
        .bind(&job_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

fn spawn_thumbnail_rebuild(
    state: AppState,
    job_id: String,
    storage_guard: tokio::sync::OwnedRwLockReadGuard<()>,
) {
    let token = CancellationToken::new();
    state.thumbnail_tasks.insert(job_id.clone(), token.clone());
    tokio::spawn(async move {
        let inner_state = state.clone();
        let inner_job_id = job_id.clone();
        let outcome = tokio::spawn(async move {
            run_thumbnail_rebuild(inner_state, inner_job_id, token, storage_guard).await
        })
        .await;
        let failure = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(error) => Some(format!("缩略图重建执行器异常退出：{error}")),
        };
        if let Some(reason) = failure {
            error!(job_id, "thumbnail rebuild failed: {reason}");
            let _ = refresh_thumbnail_rebuild_counts(&state.db, &job_id).await;
            let _ = sqlx::query("UPDATE thumbnail_rebuild_jobs SET status='failed',updated_at=?,error=? WHERE id=? AND status IN ('queued','running')")
                .bind(now())
                .bind(reason)
                .bind(&job_id)
                .execute(&state.db)
                .await;
        }
        state.thumbnail_tasks.remove(&job_id);
    });
}

async fn latest_thumbnail_rebuild(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Option<ThumbnailRebuildJob>>> {
    require_admin(&headers, &state, false).await?;
    let sql = format!(
        "SELECT {THUMBNAIL_REBUILD_JOB_COLUMNS} FROM thumbnail_rebuild_jobs ORDER BY created_at DESC LIMIT 1"
    );
    let row = sqlx::query(&sql)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(row.as_ref().map(thumbnail_job_from)))
}

async fn start_thumbnail_rebuild(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<(StatusCode, Json<ThumbnailRebuildJob>)> {
    require_admin(&headers, &state, true).await?;
    let storage_guard = state
        .storage_mutation_gate
        .clone()
        .try_read_owned()
        .map_err(|_| AppError::conflict("存储迁移或清理正在运行，请稍后重建缩略图"))?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM thumbnail_rebuild_jobs WHERE status IN ('queued','running'))",
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::internal)?;
    if active {
        return Err(AppError::conflict("已有缩略图重建任务正在运行"));
    }
    let photos = sqlx::query("SELECT id,storage_key FROM photos ORDER BY id")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?;
    let timestamp = now();
    let job_id = Uuid::new_v4().to_string();
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    sqlx::query("INSERT INTO thumbnail_rebuild_jobs(id,status,phase,total,worker_count,created_at,updated_at) VALUES(?,'queued','queued',?,?,?,?)")
        .bind(&job_id)
        .bind(photos.len() as i64)
        .bind(state.thumbnail_workers as i64)
        .bind(timestamp)
        .bind(timestamp)
        .execute(&mut *tx)
        .await
        .map_err(|error| AppError::conflict(format!("无法开始缩略图重建：{error}")))?;
    for photo in photos {
        sqlx::query("INSERT INTO thumbnail_rebuild_items(id,job_id,photo_id,storage_key,status) VALUES(?,?,?,?,'queued')")
            .bind(Uuid::new_v4().to_string())
            .bind(&job_id)
            .bind(photo.get::<String, _>("id"))
            .bind(photo.get::<String, _>("storage_key"))
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
    }
    tx.commit().await.map_err(AppError::internal)?;
    let sql =
        format!("SELECT {THUMBNAIL_REBUILD_JOB_COLUMNS} FROM thumbnail_rebuild_jobs WHERE id=?");
    let row = sqlx::query(&sql)
        .bind(&job_id)
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    let job = thumbnail_job_from(&row);
    spawn_thumbnail_rebuild(state, job_id, storage_guard);
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn cancel_thumbnail_rebuild(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    require_admin(&headers, &state, true).await?;
    let token = state
        .thumbnail_tasks
        .get(&job_id)
        .ok_or_else(|| AppError::bad("缩略图重建任务不在运行中"))?;
    token.cancel();
    Ok(StatusCode::ACCEPTED)
}

async fn resume_thumbnail_rebuild(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    require_admin(&headers, &state, true).await?;
    let storage_guard = state
        .storage_mutation_gate
        .clone()
        .try_read_owned()
        .map_err(|_| AppError::conflict("存储迁移或清理正在运行，请稍后继续缩略图任务"))?;
    let row = sqlx::query("SELECT status,phase FROM thumbnail_rebuild_jobs WHERE id=?")
        .bind(&job_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError::bad("缩略图重建任务不存在"))?;
    let status: String = row.get("status");
    let phase: String = row.get("phase");
    if !matches!(status.as_str(), "failed" | "cancelled" | "interrupted") {
        return Err(AppError::bad("此缩略图任务当前不能继续"));
    }
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    if phase == "generating" {
        sqlx::query("UPDATE thumbnail_rebuild_items SET status='queued',error=NULL WHERE job_id=? AND status IN ('failed','cancelled','processing')")
            .bind(&job_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
    } else {
        sqlx::query("UPDATE thumbnail_rebuild_items SET status='queued',error=NULL WHERE job_id=?")
            .bind(&job_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
        sqlx::query(
            "UPDATE thumbnail_rebuild_jobs SET phase='queued',cache_files_removed=0 WHERE id=?",
        )
        .bind(&job_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    }
    sqlx::query(
        "UPDATE thumbnail_rebuild_jobs SET status='queued',updated_at=?,error=NULL WHERE id=?",
    )
    .bind(now())
    .bind(&job_id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::internal)?;
    tx.commit().await.map_err(AppError::internal)?;
    refresh_thumbnail_rebuild_counts(&state.db, &job_id)
        .await
        .map_err(AppError::internal)?;
    spawn_thumbnail_rebuild(state, job_id, storage_guard);
    Ok(StatusCode::ACCEPTED)
}

async fn recover_thumbnail_rebuild(state: AppState) -> bool {
    let job_id: Option<String> = match sqlx::query_scalar("SELECT id FROM thumbnail_rebuild_jobs WHERE status='interrupted' ORDER BY created_at DESC LIMIT 1")
        .fetch_optional(&state.db)
        .await
    {
        Ok(job_id) => job_id,
        Err(error) => {
            warn!("thumbnail rebuild recovery lookup failed: {error:#}");
            return false;
        }
    };
    let Some(job_id) = job_id else {
        return false;
    };
    if let Err(error) = sqlx::query(
        "UPDATE thumbnail_rebuild_jobs SET status='queued',updated_at=?,error=NULL WHERE id=?",
    )
    .bind(now())
    .bind(&job_id)
    .execute(&state.db)
    .await
    {
        warn!(
            job_id,
            "thumbnail rebuild recovery update failed: {error:#}"
        );
        return false;
    }
    let storage_guard = state.storage_mutation_gate.clone().read_owned().await;
    spawn_thumbnail_rebuild(state, job_id, storage_guard);
    true
}

async fn photo_thumbnail(
    State(state): State<AppState>,
    AxumPath(photo_id): AxumPath<String>,
) -> ApiResult<Response> {
    let storage_key: String = sqlx::query_scalar("SELECT storage_key FROM photos WHERE id=?")
        .bind(&photo_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError {
            status: StatusCode::NOT_FOUND,
            message: "图片不存在".into(),
            clear_auth_cookies: None,
        })?;
    let thumbnail = ensure_image_derivative(&state, &photo_id, &storage_key, ImageDerivative::Grid)
        .await
        .map_err(AppError::internal)?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/png"),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        thumbnail,
    )
        .into_response())
}

async fn photo_derivative_response(
    state: AppState,
    photo_id: String,
    derivative: ImageDerivative,
) -> ApiResult<Response> {
    let storage_key: String = sqlx::query_scalar("SELECT storage_key FROM photos WHERE id=?")
        .bind(&photo_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError {
            status: StatusCode::NOT_FOUND,
            message: "图片不存在".into(),
            clear_auth_cookies: None,
        })?;
    let data = ensure_image_derivative(&state, &photo_id, &storage_key, derivative)
        .await
        .map_err(AppError::internal)?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, derivative.content_type()),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        data,
    )
        .into_response())
}

async fn photo_preview(
    State(state): State<AppState>,
    AxumPath(photo_id): AxumPath<String>,
) -> ApiResult<Response> {
    photo_derivative_response(state, photo_id, ImageDerivative::Preview).await
}

async fn photo_high(
    State(state): State<AppState>,
    AxumPath(photo_id): AxumPath<String>,
) -> ApiResult<Response> {
    photo_derivative_response(state, photo_id, ImageDerivative::High).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PhotoRenderQuery {
    format: Option<String>,
    download: Option<bool>,
}

fn exported_photo_name(original_name: &str, format: &str) -> String {
    let safe = sanitize_export_name(original_name, "photo");
    let stem = Path::new(&safe)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("photo");
    format!("{stem}.{}", if format == "jpg" { "jpg" } else { format })
}

async fn photo_render(
    State(state): State<AppState>,
    AxumPath(photo_id): AxumPath<String>,
    Query(query): Query<PhotoRenderQuery>,
) -> ApiResult<Response> {
    let row = sqlx::query("SELECT original_name,storage_key FROM photos WHERE id=?")
        .bind(&photo_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError {
            status: StatusCode::NOT_FOUND,
            message: "图片不存在".into(),
            clear_auth_cookies: None,
        })?;
    let target = normalize_format(query.format.as_deref().unwrap_or("webp"))
        .ok_or_else(|| AppError::bad("导出格式仅支持 PNG、JPG/JPEG、WEBP"))?;
    let high = ensure_image_derivative(
        &state,
        &photo_id,
        &row.get::<String, _>("storage_key"),
        ImageDerivative::High,
    )
    .await
    .map_err(AppError::internal)?;
    let _conversion_permit = timeout(
        Duration::from_secs(5),
        state.conversion_slots.clone().acquire_owned(),
    )
    .await
    .map_err(|_| AppError::too_many("当前图片导出较多，请稍后重试"))?
    .map_err(|_| AppError::internal(anyhow!("image export queue closed")))?;
    let target_for_worker = target.clone();
    let data =
        tokio::task::spawn_blocking(move || convert_webp_derivative(&high, &target_for_worker))
            .await
            .map_err(AppError::internal)?
            .map_err(AppError::internal)?;
    let filename = exported_photo_name(&row.get::<String, _>("original_name"), &target);
    let disposition = if query.download.unwrap_or(false) {
        "attachment"
    } else {
        "inline"
    };
    let mut response = Response::new(Body::from(data));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(mime_for(&target)),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "{disposition}; filename=\"photo.{}\"; filename*=UTF-8''{}",
            if target == "jpg" { "jpg" } else { &target },
            urlencoding::encode(&filename)
        ))
        .map_err(AppError::internal)?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    Ok(response)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PhotoExportRequest {
    photo_ids: Vec<String>,
    format: String,
}

async fn export_photos(
    State(state): State<AppState>,
    Json(input): Json<PhotoExportRequest>,
) -> ApiResult<Response> {
    let target = normalize_format(&input.format)
        .ok_or_else(|| AppError::bad("导出格式仅支持 PNG、JPG/JPEG、WEBP"))?;
    if input.photo_ids.is_empty() {
        return Err(AppError::bad("请至少选择一张图片"));
    }
    if input.photo_ids.len() > 500 {
        return Err(AppError::bad("一次最多打包 500 张图片"));
    }
    let unique_ids = input.photo_ids.iter().cloned().collect::<HashSet<_>>();
    if unique_ids.len() != input.photo_ids.len() {
        return Err(AppError::bad("图片选择中存在重复项"));
    }
    let mut photos = Vec::with_capacity(input.photo_ids.len());
    for photo_id in &input.photo_ids {
        let row = sqlx::query("SELECT id,original_name,storage_key FROM photos WHERE id=?")
            .bind(photo_id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::internal)?
            .ok_or_else(|| AppError {
                status: StatusCode::NOT_FOUND,
                message: format!("图片不存在：{photo_id}"),
                clear_auth_cookies: None,
            })?;
        photos.push((
            row.get::<String, _>("id"),
            row.get::<String, _>("original_name"),
            row.get::<String, _>("storage_key"),
        ));
    }
    let export_permit = timeout(
        Duration::from_secs(5),
        state.export_slots.clone().acquire_owned(),
    )
    .await
    .map_err(|_| AppError::too_many("当前打包任务较多，请稍后重试"))?
    .map_err(|_| AppError::internal(anyhow!("photo export queue closed")))?;
    let temporary_dir = std::env::temp_dir().join(format!("chronoframe-photos-{}", Uuid::new_v4()));
    let mut temporary_guard = ExportTempGuard::new(temporary_dir.clone());
    let build_result: Result<PathBuf> = async {
        tokio::fs::create_dir(&temporary_dir).await?;
        let results = stream::iter(photos.into_iter().enumerate().map(
            |(index, (photo_id, original_name, storage_key))| {
                let state = state.clone();
                let target = target.clone();
                let temporary_dir = temporary_dir.clone();
                async move {
                    let high = ensure_image_derivative(
                        &state,
                        &photo_id,
                        &storage_key,
                        ImageDerivative::High,
                    )
                    .await?;
                    let _conversion_permit = state
                        .conversion_slots
                        .clone()
                        .acquire_owned()
                        .await
                        .map_err(|_| anyhow!("图片导出工作池已关闭"))?;
                    let worker_target = target.clone();
                    let data = tokio::task::spawn_blocking(move || {
                        convert_webp_derivative(&high, &worker_target)
                    })
                    .await
                    .context("图片导出编码线程异常")??;
                    let source_path = temporary_dir.join(format!("{index}.blob"));
                    tokio::fs::write(&source_path, data).await?;
                    Ok::<(usize, PathBuf, String), anyhow::Error>((
                        index,
                        source_path,
                        exported_photo_name(&original_name, &target),
                    ))
                }
            },
        ))
        .buffer_unordered(DEFAULT_UPLOAD_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
        let mut results = results;
        results.sort_by_key(|item| item.0);
        let mut used_names = HashSet::new();
        let prepared = results
            .into_iter()
            .map(|(_, source_path, name)| PreparedArchivePhoto {
                source_path,
                archive_name: unique_export_name(name, &mut used_names),
            })
            .collect::<Vec<_>>();
        let archive_path = temporary_dir.join("chronoframe-selected-photos.zip");
        tokio::task::spawn_blocking({
            let archive_path = archive_path.clone();
            move || write_album_zip(&archive_path, &prepared)
        })
        .await
        .context("多选图片打包线程异常")??;
        Ok(archive_path)
    }
    .await;
    drop(export_permit);
    let archive_path = build_result.map_err(AppError::internal)?;
    let archive_size = tokio::fs::metadata(&archive_path)
        .await
        .map_err(AppError::internal)?
        .len();
    let archive = tokio::fs::File::open(&archive_path)
        .await
        .map_err(AppError::internal)?;
    let stream = CleanupFileStream {
        inner: ReaderStream::new(archive),
        cleanup_dir: Some(temporary_guard.take()),
    };
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&archive_size.to_string()).map_err(AppError::internal)?,
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"chronoframe-selected-photos.zip\""),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
    Ok(response)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConvertRequest {
    album_ids: Vec<String>,
    target_format: String,
}
async fn start_conversion(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ConvertRequest>,
) -> ApiResult<(StatusCode, Json<ConversionJob>)> {
    require_admin(&headers, &state, true).await?;
    let mutation_guard = state
        .storage_mutation_gate
        .clone()
        .try_read_owned()
        .map_err(|_| AppError::conflict("存储正在迁移或清理，请稍后再转换图片"))?;
    let _photo_graph_guard = state.photo_graph_lock.lock().await;
    let target = normalize_format(&input.target_format)
        .ok_or_else(|| AppError::bad("目标格式只能是 PNG、JPG/JPEG 或 WEBP"))?;
    let ids: Vec<String> = input
        .album_ids
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return Err(AppError::bad("至少选择一个相簿"));
    }
    let mut q = QueryBuilder::<Sqlite>::new(
        "SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos WHERE album_id IN (",
    );
    {
        let mut separated = q.separated(",");
        for id in &ids {
            separated.push_bind(id);
        }
    }
    q.push(") AND format != ").push_bind(&target);
    let photos: Vec<Photo> = q
        .build()
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?
        .iter()
        .map(photo_from)
        .collect();
    if photos.is_empty() {
        return Err(AppError::bad(
            "选定相簿中没有需要转换的 PNG、JPG/JPEG 或 WEBP 图片",
        ));
    }
    let timestamp = now();
    let job = ConversionJob {
        id: Uuid::new_v4().to_string(),
        status: "queued".into(),
        target_format: target,
        total: photos.len() as i64,
        completed: 0,
        succeeded: 0,
        failed: 0,
        cancelled: 0,
        created_at: timestamp,
        updated_at: timestamp,
        sources_deleted_at: None,
        source_delete_total: 0,
        source_delete_completed: 0,
        source_delete_remaining: 0,
        source_delete_failed: 0,
    };
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    sqlx::query("INSERT INTO conversion_jobs(id,status,target_format,total,completed,succeeded,failed,cancelled,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?)").bind(&job.id).bind(&job.status).bind(&job.target_format).bind(job.total).bind(0).bind(0).bind(0).bind(0).bind(timestamp).bind(timestamp).execute(&mut *tx).await.map_err(AppError::internal)?;
    for photo in photos {
        sqlx::query(
            "INSERT INTO conversion_items(id,job_id,source_photo_id,status) VALUES(?,?,?,?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&job.id)
        .bind(photo.id)
        .bind("queued")
        .execute(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    }
    tx.commit().await.map_err(AppError::internal)?;
    let token = CancellationToken::new();
    state.jobs.insert(job.id.clone(), token.clone());
    let worker_state = state.clone();
    let job_id = job.id.clone();
    tokio::spawn(async move {
        let _mutation_guard = mutation_guard;
        let inner_state = worker_state.clone();
        let inner_job_id = job_id.clone();
        let outcome =
            tokio::spawn(async move { run_job(inner_state, inner_job_id, token).await }).await;
        let failure = match outcome {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(format!("{error:#}")),
            Err(error) => Some(format!("转换执行器异常退出：{error}")),
        };
        if let Some(reason) = failure {
            error!(job_id, "conversion job failed: {reason}");
            if let Err(error) = finalize_failed_job(&worker_state.db, &job_id, &reason).await {
                error!(job_id, "failed to finalize conversion job: {error:#}");
            }
        }
        worker_state.jobs.remove(&job_id);
    });
    Ok((StatusCode::ACCEPTED, Json(job)))
}
async fn list_conversions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<Vec<ConversionJob>>> {
    require_admin(&headers, &state, false).await?;
    let rows = sqlx::query("SELECT id,status,target_format,total,completed,succeeded,failed,cancelled,created_at,updated_at,sources_deleted_at,CASE WHEN sources_deleted_at IS NULL THEN 0 ELSE succeeded END source_delete_total,CASE WHEN sources_deleted_at IS NULL THEN 0 ELSE MAX(0,succeeded-(SELECT COUNT(*) FROM source_deletion_outbox pending WHERE pending.job_id=conversion_jobs.id)) END source_delete_completed,CASE WHEN sources_deleted_at=-2 THEN (SELECT COUNT(*) FROM source_deletion_outbox pending WHERE pending.job_id=conversion_jobs.id) ELSE 0 END source_delete_remaining,CASE WHEN sources_deleted_at=-2 THEN (SELECT COUNT(*) FROM source_deletion_outbox pending WHERE pending.job_id=conversion_jobs.id AND pending.last_error IS NOT NULL) ELSE 0 END source_delete_failed FROM conversion_jobs ORDER BY created_at DESC LIMIT 100").fetch_all(&state.db).await.map_err(AppError::internal)?;
    Ok(Json(rows.iter().map(job_from).collect()))
}
#[derive(Deserialize)]
struct JobQuery {
    items: Option<bool>,
}
async fn get_conversion(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    Query(query): Query<JobQuery>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, false).await?;
    let row = sqlx::query("SELECT id,status,target_format,total,completed,succeeded,failed,cancelled,created_at,updated_at,sources_deleted_at,CASE WHEN sources_deleted_at IS NULL THEN 0 ELSE succeeded END source_delete_total,CASE WHEN sources_deleted_at IS NULL THEN 0 ELSE MAX(0,succeeded-(SELECT COUNT(*) FROM source_deletion_outbox pending WHERE pending.job_id=conversion_jobs.id)) END source_delete_completed,CASE WHEN sources_deleted_at=-2 THEN (SELECT COUNT(*) FROM source_deletion_outbox pending WHERE pending.job_id=conversion_jobs.id) ELSE 0 END source_delete_remaining,CASE WHEN sources_deleted_at=-2 THEN (SELECT COUNT(*) FROM source_deletion_outbox pending WHERE pending.job_id=conversion_jobs.id AND pending.last_error IS NOT NULL) ELSE 0 END source_delete_failed FROM conversion_jobs WHERE id=?").bind(&job_id).fetch_optional(&state.db).await.map_err(AppError::internal)?.ok_or_else(|| AppError { status: StatusCode::NOT_FOUND, message: "转换任务不存在".into(), clear_auth_cookies: None })?;
    let items = if query.items.unwrap_or(true) {
        sqlx::query("SELECT i.id,i.source_photo_id,COALESCE(p.original_name,'已删除原图') source_name,i.status,i.target_photo_id,i.error FROM conversion_items i LEFT JOIN photos p ON p.id=i.source_photo_id WHERE i.job_id=? ORDER BY source_name").bind(&job_id).fetch_all(&state.db).await.map_err(AppError::internal)?.iter().map(|r| ConversionItem { id:r.get("id"), source_photo_id:r.get::<Option<String>, _>("source_photo_id").unwrap_or_default(), source_name:r.get("source_name"), status:r.get("status"), target_photo_id:r.get("target_photo_id"), error:r.get("error") }).collect::<Vec<_>>()
    } else {
        vec![]
    };
    Ok(Json(
        serde_json::json!({"job": job_from(&row), "items": items}),
    ))
}
async fn cancel_conversion(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    require_admin(&headers, &state, true).await?;
    if let Some(token) = state.jobs.get(&job_id) {
        token.cancel();
    } else {
        return Err(AppError::bad("任务不在运行中，无法取消"));
    }
    Ok(StatusCode::ACCEPTED)
}
async fn confirm_delete_sources(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    require_admin(&headers, &state, true).await?;
    let mutation_guard = state
        .storage_mutation_gate
        .clone()
        .try_read_owned()
        .map_err(|_| AppError::conflict("存储正在迁移或清理，请稍后再删除旧格式图片"))?;
    let photo_graph_guard = state.photo_graph_lock.clone().lock_owned().await;
    let job =
        sqlx::query("SELECT status,succeeded,sources_deleted_at FROM conversion_jobs WHERE id=?")
            .bind(&job_id)
            .fetch_optional(&state.db)
            .await
            .map_err(AppError::internal)?
            .ok_or_else(|| AppError {
                status: StatusCode::NOT_FOUND,
                message: "转换任务不存在".into(),
                clear_auth_cookies: None,
            })?;
    let status: String = job.get("status");
    let succeeded: i64 = job.get("succeeded");
    let sources_deleted_at: Option<i64> = job.get("sources_deleted_at");
    if !matches!(
        status.as_str(),
        "completed" | "failed" | "cancelled" | "interrupted"
    ) || succeeded == 0
    {
        return Err(AppError::bad("任务尚无可安全确认的转换结果"));
    }
    if sources_deleted_at.is_some() {
        return Err(AppError::bad("此任务的旧图删除已确认过或正在执行"));
    }
    let active_reference: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM conversion_items active WHERE active.status IN ('queued','processing') AND active.source_photo_id IN (SELECT source_photo_id FROM conversion_items WHERE job_id=? AND status='succeeded'))").bind(&job_id).fetch_one(&state.db).await.map_err(AppError::internal)?;
    if active_reference {
        return Err(AppError::bad(
            "仍有转换任务正在使用这些原图，请等待或安全中断后再删除",
        ));
    }
    let rows = sqlx::query("SELECT p.id,p.storage_key,t.storage_key target_key,t.format target_format,t.byte_size target_size FROM conversion_items i JOIN photos p ON p.id=i.source_photo_id LEFT JOIN photos t ON t.id=i.target_photo_id WHERE i.job_id=? AND i.status='succeeded'").bind(&job_id).fetch_all(&state.db).await.map_err(AppError::internal)?;
    if rows.len() != succeeded as usize {
        return Err(AppError::conflict(
            "部分转换记录已发生变化，无法安全建立旧图删除任务",
        ));
    }
    let mut prepared = vec![];
    for row in rows {
        let id: String = row.get("id");
        let source_key: String = row.get("storage_key");
        let target_key: Option<String> = row.get("target_key");
        let target_format: Option<String> = row.get("target_format");
        let target_size: Option<i64> = row.get("target_size");
        let (Some(target_key), Some(target_format), Some(target_size)) =
            (target_key, target_format, target_size)
        else {
            return Err(AppError::conflict(
                "转换后的新图记录不完整，未授权删除任何旧图",
            ));
        };
        prepared.push((id, source_key, target_key, target_format, target_size));
    }

    // Persist every authorized deletion before touching a source object. The -2 state means the
    // durable outbox is complete and may be replayed safely after SIGKILL or a database outage.
    let mut tx = state.db.begin().await.map_err(AppError::internal)?;
    let prepared_state = sqlx::query("UPDATE conversion_jobs SET sources_deleted_at=-2,updated_at=? WHERE id=? AND sources_deleted_at IS NULL").bind(now()).bind(&job_id).execute(&mut *tx).await.map_err(AppError::internal)?.rows_affected();
    if prepared_state != 1 {
        return Err(AppError::bad("旧图删除已由其他请求确认，请刷新任务"));
    }
    for (id, source_key, target_key, target_format, target_size) in &prepared {
        sqlx::query("INSERT OR IGNORE INTO source_deletion_outbox(job_id,source_photo_id,source_key,target_key,target_format,target_size,created_at) VALUES(?,?,?,?,?,?,?)").bind(&job_id).bind(id).bind(source_key).bind(target_key).bind(target_format).bind(target_size).bind(now()).execute(&mut *tx).await.map_err(AppError::internal)?;
    }
    tx.commit().await.map_err(AppError::internal)?;
    let queued = prepared.len();
    let worker_state = state.clone();
    let worker_job_id = job_id.clone();
    tokio::spawn(async move {
        let _mutation_guard = mutation_guard;
        let _photo_graph_guard = photo_graph_guard;
        let _storage_guard = worker_state.storage.gate.read().await;
        match drain_source_deletion_outbox(
            &worker_state.storage,
            &worker_state.db,
            Some(&worker_job_id),
        )
        .await
        {
            Ok(result) if result.removed > 0 || !result.failures.is_empty() => info!(
                job_id = %worker_job_id,
                removed = result.removed,
                failures = result.failures.len(),
                "background source deletion batch finished"
            ),
            Ok(_) => {}
            Err(error) => {
                warn!(job_id = %worker_job_id, "background source deletion deferred: {error:#}")
            }
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "queued",
            "total": queued,
            "removed": 0,
            "failures": []
        })),
    ))
}

async fn run_job(state: AppState, job_id: String, token: CancellationToken) -> Result<()> {
    sqlx::query(
        "UPDATE conversion_jobs SET status='running',updated_at=? WHERE id=? AND status='queued'",
    )
    .bind(now())
    .bind(&job_id)
    .execute(&state.db)
    .await?;
    let items = sqlx::query(
        "SELECT id,source_photo_id FROM conversion_items WHERE job_id=? AND status='queued'",
    )
    .bind(&job_id)
    .fetch_all(&state.db)
    .await?;
    let items: Vec<(String, String)> = items
        .iter()
        .map(|r| (r.get("id"), r.get("source_photo_id")))
        .collect();
    let workers = state.workers;
    stream::iter(items).for_each_concurrent(workers, |(item_id, photo_id)| { let state = state.clone(); let job_id = job_id.clone(); let token = token.clone(); async move {
        let permit = tokio::select! {
            _ = token.cancelled() => { let _ = mark_item(&state.db, &job_id, &item_id, "cancelled", None, None).await; return; }
            permit = state.conversion_slots.clone().acquire_owned() => match permit { Ok(permit) => permit, Err(_) => { let _ = mark_item(&state.db, &job_id, &item_id, "failed", None, Some("全局转换工作池已关闭")).await; return; } }
        };
        let _permit = permit;
        if let Err(e) = convert_one(&state, &job_id, &item_id, &photo_id, &token).await { warn!(job_id, item_id, "conversion item failed: {e:#}"); let _ = mark_item(&state.db, &job_id, &item_id, "failed", None, Some(&e.to_string())).await; }
    } }).await;
    if token.is_cancelled() {
        sqlx::query("UPDATE conversion_items SET status='cancelled',error=COALESCE(error,'管理员安全中断任务') WHERE job_id=? AND status IN ('queued','processing')").bind(&job_id).execute(&state.db).await?;
    } else {
        sqlx::query("UPDATE conversion_items SET status='failed',error=COALESCE(error,'工作线程未生成终态') WHERE job_id=? AND status IN ('queued','processing')").bind(&job_id).execute(&state.db).await?;
    }
    refresh_job_counts(&state.db, &job_id).await?;
    let counts = sqlx::query(
        "SELECT total,completed,succeeded,failed,cancelled FROM conversion_jobs WHERE id=?",
    )
    .bind(&job_id)
    .fetch_one(&state.db)
    .await?;
    let total: i64 = counts.get("total");
    let completed: i64 = counts.get("completed");
    let failed: i64 = counts.get("failed");
    if completed != total {
        bail!("任务终态计数不完整：{completed}/{total}");
    }
    let status = if token.is_cancelled() {
        "cancelled"
    } else if failed > 0 {
        "failed"
    } else {
        "completed"
    };
    sqlx::query("UPDATE conversion_jobs SET status=?,updated_at=? WHERE id=?")
        .bind(status)
        .bind(now())
        .bind(&job_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn finalize_failed_job(db: &SqlitePool, job_id: &str, reason: &str) -> Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("UPDATE conversion_items SET status='failed',error=COALESCE(error,?) WHERE job_id=? AND status IN ('queued','processing')").bind(reason).bind(job_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE conversion_jobs SET status='failed',completed=(SELECT COUNT(*) FROM conversion_items WHERE job_id=? AND status IN ('succeeded','failed','cancelled')),succeeded=(SELECT COUNT(*) FROM conversion_items WHERE job_id=? AND status='succeeded'),failed=(SELECT COUNT(*) FROM conversion_items WHERE job_id=? AND status='failed'),cancelled=(SELECT COUNT(*) FROM conversion_items WHERE job_id=? AND status='cancelled'),updated_at=? WHERE id=?").bind(job_id).bind(job_id).bind(job_id).bind(job_id).bind(now()).bind(job_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
async fn mark_item(
    db: &SqlitePool,
    job_id: &str,
    item_id: &str,
    status: &str,
    target: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE conversion_items SET status=?,target_photo_id=?,error=? WHERE id=? AND status NOT IN ('succeeded','failed','cancelled')").bind(status).bind(target).bind(error).bind(item_id).execute(db).await?;
    refresh_job_counts(db, job_id).await
}
async fn refresh_job_counts(db: &SqlitePool, job_id: &str) -> Result<()> {
    sqlx::query("UPDATE conversion_jobs SET completed=(SELECT COUNT(*) FROM conversion_items WHERE job_id=? AND status IN ('succeeded','failed','cancelled')),succeeded=(SELECT COUNT(*) FROM conversion_items WHERE job_id=? AND status='succeeded'),failed=(SELECT COUNT(*) FROM conversion_items WHERE job_id=? AND status='failed'),cancelled=(SELECT COUNT(*) FROM conversion_items WHERE job_id=? AND status='cancelled'),updated_at=? WHERE id=?").bind(job_id).bind(job_id).bind(job_id).bind(job_id).bind(now()).bind(job_id).execute(db).await?;
    Ok(())
}
async fn convert_one(
    state: &AppState,
    job_id: &str,
    item_id: &str,
    photo_id: &str,
    token: &CancellationToken,
) -> Result<()> {
    let _storage_guard = state.storage.gate.read().await;
    if token.is_cancelled() {
        return mark_item(&state.db, job_id, item_id, "cancelled", None, None).await;
    }
    sqlx::query("UPDATE conversion_items SET status='processing' WHERE id=? AND status='queued'")
        .bind(item_id)
        .execute(&state.db)
        .await?;
    let row = sqlx::query("SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos WHERE id=?").bind(photo_id).fetch_one(&state.db).await?;
    let source = photo_from(&row);
    let storage = state.storage.store().await?;
    let input = tokio::select! {
        _ = token.cancelled() => return mark_item(&state.db, job_id, item_id, "cancelled", None, None).await,
        result = timeout(STORAGE_IO_TIMEOUT, storage.get(&source.storage_key)) => result.context("读取原图超时")??,
    };
    if token.is_cancelled() {
        return mark_item(&state.db, job_id, item_id, "cancelled", None, None).await;
    }
    let target_format: String =
        sqlx::query_scalar("SELECT target_format FROM conversion_jobs WHERE id=?")
            .bind(job_id)
            .fetch_one(&state.db)
            .await?;
    let format = target_format.clone();
    let encoded = tokio::task::spawn_blocking(move || encode_image(&input, &format))
        .await
        .context("image worker panicked")??;
    if token.is_cancelled() {
        return mark_item(&state.db, job_id, item_id, "cancelled", None, None).await;
    }
    let new_id = Uuid::new_v4().to_string();
    let key = format!(
        "albums/{}/conversions/{job_id}/{new_id}.{target_format}",
        source.album_id
    );
    let content_type = mime_for(&target_format).to_string();
    let digest = Sha256::digest(&encoded.data);
    let digest_prefix = hex_digest(&digest)[..8].to_string();
    let byte_size = encoded.data.len() as i64;
    let width = encoded.width;
    let height = encoded.height;
    register_pending_blob(&state.db, &key).await?;
    // Atomic object mutations are allowed to reach their bounded deadline; dropping MOVE/CopyObject midway would make the commit state unknowable.
    storage
        .put_atomic(&key, &content_type, encoded.data)
        .await?;
    if token.is_cancelled() {
        let cleanup_error = cleanup_pending_blob(&storage, &state.db, &key)
            .await
            .err()
            .map(|error| format!("转换已取消；对象将在后台继续清理：{error:#}"));
        return mark_item(
            &state.db,
            job_id,
            item_id,
            "cancelled",
            None,
            cleanup_error.as_deref(),
        )
        .await;
    }
    let name = renamed(&source.original_name, &target_format);
    let photo = Photo {
        id: new_id,
        album_id: source.album_id,
        original_name: format!("{name} [{digest_prefix}]"),
        storage_key: key,
        format: target_format,
        content_type,
        byte_size,
        width,
        height,
        created_at: now(),
    };
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO photos(id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at) VALUES(?,?,?,?,?,?,?,?,?,?)").bind(&photo.id).bind(&photo.album_id).bind(&photo.original_name).bind(&photo.storage_key).bind(&photo.format).bind(&photo.content_type).bind(photo.byte_size).bind(photo.width).bind(photo.height).bind(photo.created_at).execute(&mut *tx).await?;
    sqlx::query("UPDATE conversion_items SET status='succeeded',target_photo_id=? WHERE id=?")
        .bind(&photo.id)
        .bind(item_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM pending_blobs WHERE key=?")
        .bind(&photo.storage_key)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    spawn_thumbnail_generation(
        (*state).clone(),
        vec![(photo.id.clone(), photo.storage_key.clone())],
    );
    refresh_job_counts(&state.db, job_id).await
}
struct EncodedImage {
    data: Vec<u8>,
    width: i64,
    height: i64,
}

fn encode_image(input: &[u8], target: &str) -> Result<EncodedImage> {
    let image = image::load_from_memory(input)?;
    let width = image.width() as i64;
    let height = image.height() as i64;
    let mut output = Cursor::new(Vec::new());
    image.write_to(
        &mut output,
        match target {
            "png" => ImageFormat::Png,
            "jpg" => ImageFormat::Jpeg,
            "webp" => ImageFormat::WebP,
            _ => bail!("invalid target format"),
        },
    )?;
    Ok(EncodedImage {
        data: output.into_inner(),
        width,
        height,
    })
}

fn resize_to_longest_edge(image: &image::DynamicImage, longest_edge: u32) -> image::DynamicImage {
    let width = image.width();
    let height = image.height();
    let longest = width.max(height);
    if longest <= longest_edge {
        return image.clone();
    }
    let target_width = ((width as u64 * longest_edge as u64) / longest as u64).max(1) as u32;
    let target_height = ((height as u64 * longest_edge as u64) / longest as u64).max(1) as u32;
    image.resize_exact(
        target_width,
        target_height,
        image::imageops::FilterType::Lanczos3,
    )
}

fn encode_png_thumbnail_from_image(
    image: &image::DynamicImage,
    longest_edge: u32,
) -> Result<Vec<u8>> {
    if longest_edge == 0 {
        bail!("thumbnail edge must be positive");
    }
    let thumbnail = resize_to_longest_edge(image, longest_edge);
    let mut output = Cursor::new(Vec::new());
    thumbnail.write_to(&mut output, ImageFormat::Png)?;
    Ok(output.into_inner())
}

#[cfg(test)]
fn encode_thumbnail(input: &[u8], longest_edge: u32) -> Result<Vec<u8>> {
    let image = image::load_from_memory(input)?;
    encode_png_thumbnail_from_image(&image, longest_edge)
}

fn encode_limited_webp_from_image(
    image: &image::DynamicImage,
    longest_edge: u32,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    if longest_edge == 0 || max_bytes < 1024 {
        bail!("invalid WebP derivative limits");
    }
    let mut resized = resize_to_longest_edge(image, longest_edge);
    let qualities = [
        92.0_f32, 86.0, 80.0, 74.0, 68.0, 60.0, 52.0, 44.0, 36.0, 28.0,
    ];
    let mut smallest = Vec::new();
    for _ in 0..12 {
        let rgba = resized.to_rgba8();
        for quality in qualities {
            let encoded = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
                .encode(quality)
                .to_vec();
            if smallest.is_empty() || encoded.len() < smallest.len() {
                smallest = encoded.clone();
            }
            if encoded.len() <= max_bytes {
                return Ok(encoded);
            }
        }
        if resized.width() <= 2 && resized.height() <= 2 {
            break;
        }
        let next_width = ((resized.width() as f32 * 0.82).round() as u32).max(1);
        let next_height = ((resized.height() as f32 * 0.82).round() as u32).max(1);
        resized = resized.resize_exact(
            next_width,
            next_height,
            image::imageops::FilterType::Lanczos3,
        );
    }
    if smallest.len() <= max_bytes {
        Ok(smallest)
    } else {
        bail!("无法把 WebP 派生图压缩到 {} 字节以内", max_bytes)
    }
}

fn encode_derivative(input: &[u8], derivative: ImageDerivative) -> Result<Vec<u8>> {
    let image = image::load_from_memory(input)?;
    match derivative {
        ImageDerivative::Grid => {
            encode_png_thumbnail_from_image(&image, GRID_THUMBNAIL_LONGEST_EDGE)
        }
        ImageDerivative::Preview => encode_limited_webp_from_image(
            &image,
            VIEW_PREVIEW_LONGEST_EDGE,
            VIEW_PREVIEW_MAX_BYTES,
        ),
        ImageDerivative::High => {
            encode_limited_webp_from_image(&image, VIEW_HIGH_LONGEST_EDGE, VIEW_HIGH_MAX_BYTES)
        }
    }
}

fn encode_all_derivatives(input: &[u8]) -> Result<Vec<(ImageDerivative, Vec<u8>)>> {
    let image = image::load_from_memory(input)?;
    Ok(vec![
        (
            ImageDerivative::Grid,
            encode_png_thumbnail_from_image(&image, GRID_THUMBNAIL_LONGEST_EDGE)?,
        ),
        (
            ImageDerivative::Preview,
            encode_limited_webp_from_image(
                &image,
                VIEW_PREVIEW_LONGEST_EDGE,
                VIEW_PREVIEW_MAX_BYTES,
            )?,
        ),
        (
            ImageDerivative::High,
            encode_limited_webp_from_image(&image, VIEW_HIGH_LONGEST_EDGE, VIEW_HIGH_MAX_BYTES)?,
        ),
    ])
}

fn convert_webp_derivative(input: &[u8], target: &str) -> Result<Vec<u8>> {
    if target == "webp" {
        return Ok(input.to_vec());
    }
    let image = image::load_from_memory_with_format(input, ImageFormat::WebP)?;
    let mut output = Cursor::new(Vec::new());
    match target {
        "png" => image.write_to(&mut output, ImageFormat::Png)?,
        "jpg" => {
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 92);
            encoder.encode_image(&image)?;
        }
        _ => bail!("invalid export format"),
    }
    Ok(output.into_inner())
}
fn normalize_format(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "png" => Some("png".into()),
        "jpg" | "jpeg" => Some("jpg".into()),
        "webp" => Some("webp".into()),
        _ => None,
    }
}
fn file_format(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(|x| x.to_str())
        .and_then(normalize_format)
}
fn mime_for(format: &str) -> &'static str {
    match format {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}
fn renamed(name: &str, format: &str) -> String {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|x| x.to_str())
        .unwrap_or("image");
    format!("{stem}.{format}")
}
fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn setup_database(pool: &SqlitePool) -> Result<()> {
    sqlx::query("PRAGMA foreign_keys = ON; CREATE TABLE IF NOT EXISTS app_settings (key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS administrators (id INTEGER PRIMARY KEY CHECK(id=1),username TEXT NOT NULL UNIQUE,password_hash TEXT NOT NULL,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS admin_sessions (token_hash TEXT PRIMARY KEY,administrator_id INTEGER NOT NULL REFERENCES administrators(id) ON DELETE CASCADE CHECK(administrator_id=1),csrf_hash TEXT NOT NULL,created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_admin_sessions_expires_at ON admin_sessions(expires_at); CREATE TABLE IF NOT EXISTS albums (id TEXT PRIMARY KEY,name TEXT NOT NULL,description TEXT NOT NULL DEFAULT '',created_at INTEGER NOT NULL,display_created_date TEXT,photo_date_start TEXT,photo_date_end TEXT,position INTEGER NOT NULL DEFAULT 0); CREATE TABLE IF NOT EXISTS photos (id TEXT PRIMARY KEY,album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,original_name TEXT NOT NULL,storage_key TEXT NOT NULL UNIQUE,format TEXT NOT NULL CHECK(format IN ('png','jpg','webp')),content_type TEXT NOT NULL,byte_size INTEGER NOT NULL,width INTEGER NOT NULL DEFAULT 0,height INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS pending_blobs (key TEXT PRIMARY KEY,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS photo_deletion_outbox (photo_id TEXT PRIMARY KEY,storage_key TEXT NOT NULL UNIQUE,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS conversion_jobs (id TEXT PRIMARY KEY,status TEXT NOT NULL,target_format TEXT NOT NULL,total INTEGER NOT NULL,completed INTEGER NOT NULL DEFAULT 0,succeeded INTEGER NOT NULL DEFAULT 0,failed INTEGER NOT NULL DEFAULT 0,cancelled INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,sources_deleted_at INTEGER); CREATE TABLE IF NOT EXISTS conversion_items (id TEXT PRIMARY KEY,job_id TEXT NOT NULL REFERENCES conversion_jobs(id) ON DELETE CASCADE,source_photo_id TEXT REFERENCES photos(id) ON DELETE SET NULL,target_photo_id TEXT REFERENCES photos(id) ON DELETE SET NULL,status TEXT NOT NULL,error TEXT); CREATE TABLE IF NOT EXISTS source_deletion_outbox (job_id TEXT NOT NULL REFERENCES conversion_jobs(id) ON DELETE CASCADE,source_photo_id TEXT NOT NULL,source_key TEXT NOT NULL,target_key TEXT NOT NULL,target_format TEXT NOT NULL,target_size INTEGER NOT NULL,created_at INTEGER NOT NULL,attempts INTEGER NOT NULL DEFAULT 0,last_error TEXT,next_retry_at INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(job_id,source_photo_id)); CREATE TABLE IF NOT EXISTS storage_migration_jobs (id TEXT PRIMARY KEY,status TEXT NOT NULL,source_backend TEXT NOT NULL,target_backend TEXT NOT NULL,total INTEGER NOT NULL,completed INTEGER NOT NULL DEFAULT 0,succeeded INTEGER NOT NULL DEFAULT 0,failed INTEGER NOT NULL DEFAULT 0,cancelled INTEGER NOT NULL DEFAULT 0,cleanup_status TEXT NOT NULL DEFAULT 'not_ready',cleanup_completed INTEGER NOT NULL DEFAULT 0,cleanup_failed INTEGER NOT NULL DEFAULT 0,source_config TEXT NOT NULL,target_config TEXT NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,activated_at INTEGER,error TEXT); CREATE TABLE IF NOT EXISTS storage_migration_items (id TEXT PRIMARY KEY,job_id TEXT NOT NULL REFERENCES storage_migration_jobs(id) ON DELETE CASCADE,photo_id TEXT NOT NULL,storage_key TEXT NOT NULL,content_type TEXT NOT NULL,byte_size INTEGER NOT NULL,status TEXT NOT NULL DEFAULT 'queued',sha256 TEXT,error TEXT,source_deleted_at INTEGER,cleanup_error TEXT,UNIQUE(job_id,photo_id)); CREATE INDEX IF NOT EXISTS idx_storage_migration_items_job_status ON storage_migration_items(job_id,status); CREATE TABLE IF NOT EXISTS thumbnail_rebuild_jobs (id TEXT PRIMARY KEY,status TEXT NOT NULL,phase TEXT NOT NULL DEFAULT 'queued',total INTEGER NOT NULL,completed INTEGER NOT NULL DEFAULT 0,succeeded INTEGER NOT NULL DEFAULT 0,failed INTEGER NOT NULL DEFAULT 0,skipped INTEGER NOT NULL DEFAULT 0,cancelled INTEGER NOT NULL DEFAULT 0,cache_files_removed INTEGER NOT NULL DEFAULT 0,worker_count INTEGER NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,error TEXT); CREATE TABLE IF NOT EXISTS thumbnail_rebuild_items (id TEXT PRIMARY KEY,job_id TEXT NOT NULL REFERENCES thumbnail_rebuild_jobs(id) ON DELETE CASCADE,photo_id TEXT NOT NULL,storage_key TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'queued',error TEXT,UNIQUE(job_id,photo_id)); CREATE INDEX IF NOT EXISTS idx_thumbnail_rebuild_items_job_status ON thumbnail_rebuild_items(job_id,status); CREATE UNIQUE INDEX IF NOT EXISTS idx_thumbnail_rebuild_one_active ON thumbnail_rebuild_jobs((1)) WHERE status IN ('queued','running');").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS s3_cleanup_jobs (id TEXT PRIMARY KEY,status TEXT NOT NULL,phase TEXT NOT NULL,scanned_objects INTEGER NOT NULL DEFAULT 0,protected_objects INTEGER NOT NULL DEFAULT 0,total INTEGER NOT NULL DEFAULT 0,completed INTEGER NOT NULL DEFAULT 0,deleted INTEGER NOT NULL DEFAULT 0,failed INTEGER NOT NULL DEFAULT 0,skipped INTEGER NOT NULL DEFAULT 0,bytes_found INTEGER NOT NULL DEFAULT 0,bytes_deleted INTEGER NOT NULL DEFAULT 0,worker_count INTEGER NOT NULL,location_key TEXT NOT NULL,managed_prefix TEXT NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,error TEXT); CREATE TABLE IF NOT EXISTS s3_cleanup_items (id TEXT PRIMARY KEY,job_id TEXT NOT NULL REFERENCES s3_cleanup_jobs(id) ON DELETE CASCADE,object_key TEXT NOT NULL,logical_key TEXT NOT NULL,byte_size INTEGER NOT NULL,last_modified INTEGER NOT NULL,status TEXT NOT NULL DEFAULT 'queued',error TEXT,UNIQUE(job_id,object_key)); CREATE INDEX IF NOT EXISTS idx_s3_cleanup_items_job_status ON s3_cleanup_items(job_id,status); CREATE UNIQUE INDEX IF NOT EXISTS idx_s3_cleanup_one_active ON s3_cleanup_jobs((1)) WHERE status='running';")
        .execute(pool)
        .await?;
    album_covers::setup(pool).await?;
    let album_columns: HashSet<String> = sqlx::query("PRAGMA table_info(albums)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|row| row.get("name"))
        .collect();
    for (column, migration) in [
        (
            "description",
            "ALTER TABLE albums ADD COLUMN description TEXT NOT NULL DEFAULT ''",
        ),
        (
            "display_created_date",
            "ALTER TABLE albums ADD COLUMN display_created_date TEXT",
        ),
        (
            "photo_date_start",
            "ALTER TABLE albums ADD COLUMN photo_date_start TEXT",
        ),
        (
            "photo_date_end",
            "ALTER TABLE albums ADD COLUMN photo_date_end TEXT",
        ),
        (
            "position",
            "ALTER TABLE albums ADD COLUMN position INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        if !album_columns.contains(column) {
            sqlx::query(migration).execute(pool).await?;
        }
    }
    if !album_columns.contains("position") {
        sqlx::query("WITH ranked AS (SELECT id,ROW_NUMBER() OVER (ORDER BY created_at DESC,id ASC)-1 AS new_position FROM albums) UPDATE albums SET position=(SELECT new_position FROM ranked WHERE ranked.id=albums.id)")
            .execute(pool)
            .await?;
    }
    let photo_columns: HashSet<String> = sqlx::query("PRAGMA table_info(photos)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|row| row.get("name"))
        .collect();
    if !photo_columns.contains("width") {
        sqlx::query("ALTER TABLE photos ADD COLUMN width INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }
    if !photo_columns.contains("height") {
        sqlx::query("ALTER TABLE photos ADD COLUMN height INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }
    let source_deletion_columns: HashSet<String> =
        sqlx::query("PRAGMA table_info(source_deletion_outbox)")
            .fetch_all(pool)
            .await?
            .iter()
            .map(|row| row.get("name"))
            .collect();
    for (column, migration) in [
        (
            "attempts",
            "ALTER TABLE source_deletion_outbox ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "last_error",
            "ALTER TABLE source_deletion_outbox ADD COLUMN last_error TEXT",
        ),
        (
            "next_retry_at",
            "ALTER TABLE source_deletion_outbox ADD COLUMN next_retry_at INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        if !source_deletion_columns.contains(column) {
            sqlx::query(migration).execute(pool).await?;
        }
    }
    for (key, value) in [
        ("storage_backend", "local"),
        ("storage_local_path", "./data/storage"),
        ("storage_webdav_url", ""),
        ("storage_webdav_username", ""),
        ("storage_webdav_prefix", "chronoframe"),
        ("storage_webdav_password", ""),
        ("storage_s3_endpoint", ""),
        ("storage_s3_region", "us-east-1"),
        ("storage_s3_bucket", ""),
        ("storage_s3_access_key", ""),
        ("storage_s3_secret_key", ""),
        ("storage_s3_prefix", "chronoframe"),
        ("site_title", DEFAULT_SITE_TITLE),
        ("site_slogan", DEFAULT_SITE_SLOGAN),
        ("site_author", DEFAULT_SITE_AUTHOR),
        ("site_avatar_url", DEFAULT_SITE_AVATAR_URL),
        ("site_theme", DEFAULT_SITE_THEME),
    ] {
        sqlx::query("INSERT INTO app_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO NOTHING")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await?;
    }
    let source_delete_rule: Option<String> = sqlx::query_scalar("SELECT \"on_delete\" FROM pragma_foreign_key_list('conversion_items') WHERE \"from\"='source_photo_id'").fetch_optional(pool).await?;
    let target_delete_rule: Option<String> = sqlx::query_scalar("SELECT \"on_delete\" FROM pragma_foreign_key_list('conversion_items') WHERE \"from\"='target_photo_id'").fetch_optional(pool).await?;
    if source_delete_rule.as_deref() != Some("SET NULL")
        || target_delete_rule.as_deref() != Some("SET NULL")
    {
        // SQLite cannot alter a foreign-key action in place. Rebuild only this small metadata table, preserving every job record.
        sqlx::query("PRAGMA foreign_keys=OFF; BEGIN; ALTER TABLE conversion_items RENAME TO conversion_items_legacy; CREATE TABLE conversion_items (id TEXT PRIMARY KEY,job_id TEXT NOT NULL REFERENCES conversion_jobs(id) ON DELETE CASCADE,source_photo_id TEXT REFERENCES photos(id) ON DELETE SET NULL,target_photo_id TEXT REFERENCES photos(id) ON DELETE SET NULL,status TEXT NOT NULL,error TEXT); INSERT INTO conversion_items(id,job_id,source_photo_id,target_photo_id,status,error) SELECT id,job_id,source_photo_id,target_photo_id,status,error FROM conversion_items_legacy; DROP TABLE conversion_items_legacy; COMMIT; PRAGMA foreign_keys=ON;").execute(pool).await?;
    }
    // A restart never resumes abandoned workers: their committed results remain safe, all unfinished work is explicitly marked interrupted.
    sqlx::query("UPDATE conversion_jobs SET sources_deleted_at=NULL WHERE sources_deleted_at=-1")
        .execute(pool)
        .await?;
    sqlx::query("UPDATE conversion_items SET status='cancelled',error='服务器重启，任务已安全中断' WHERE status IN ('queued','processing')").execute(pool).await?;
    sqlx::query("UPDATE conversion_jobs SET status='interrupted',completed=(SELECT COUNT(*) FROM conversion_items i WHERE i.job_id=conversion_jobs.id AND i.status IN ('succeeded','failed','cancelled')),succeeded=(SELECT COUNT(*) FROM conversion_items i WHERE i.job_id=conversion_jobs.id AND i.status='succeeded'),failed=(SELECT COUNT(*) FROM conversion_items i WHERE i.job_id=conversion_jobs.id AND i.status='failed'),cancelled=(SELECT COUNT(*) FROM conversion_items i WHERE i.job_id=conversion_jobs.id AND i.status='cancelled'),updated_at=? WHERE status IN ('queued','running')").bind(now()).execute(pool).await?;
    sqlx::query("UPDATE storage_migration_items SET status='queued',error='服务器重启，复制任务已安全中断' WHERE status='processing'")
        .execute(pool)
        .await?;
    sqlx::query("UPDATE storage_migration_jobs SET status='interrupted',completed=(SELECT COUNT(*) FROM storage_migration_items i WHERE i.job_id=storage_migration_jobs.id AND i.status IN ('succeeded','failed','cancelled')),succeeded=(SELECT COUNT(*) FROM storage_migration_items i WHERE i.job_id=storage_migration_jobs.id AND i.status='succeeded'),failed=(SELECT COUNT(*) FROM storage_migration_items i WHERE i.job_id=storage_migration_jobs.id AND i.status='failed'),cancelled=(SELECT COUNT(*) FROM storage_migration_items i WHERE i.job_id=storage_migration_jobs.id AND i.status='cancelled'),updated_at=?,error='服务器重启，迁移任务已安全中断' WHERE status IN ('queued','running')")
        .bind(now())
        .execute(pool)
        .await?;
    sqlx::query("UPDATE storage_migration_jobs SET cleanup_status='interrupted',updated_at=?,error='服务器重启，旧存储清理已安全中断' WHERE cleanup_status='cleaning'")
        .bind(now())
        .execute(pool)
        .await?;
    sqlx::query("UPDATE thumbnail_rebuild_items SET status='queued',error='服务器重启，生成任务已安全中断' WHERE status='processing'")
        .execute(pool)
        .await?;
    sqlx::query("UPDATE thumbnail_rebuild_jobs SET status='interrupted',completed=(SELECT COUNT(*) FROM thumbnail_rebuild_items i WHERE i.job_id=thumbnail_rebuild_jobs.id AND i.status IN ('succeeded','failed','skipped','cancelled')),succeeded=(SELECT COUNT(*) FROM thumbnail_rebuild_items i WHERE i.job_id=thumbnail_rebuild_jobs.id AND i.status='succeeded'),failed=(SELECT COUNT(*) FROM thumbnail_rebuild_items i WHERE i.job_id=thumbnail_rebuild_jobs.id AND i.status='failed'),skipped=(SELECT COUNT(*) FROM thumbnail_rebuild_items i WHERE i.job_id=thumbnail_rebuild_jobs.id AND i.status='skipped'),cancelled=(SELECT COUNT(*) FROM thumbnail_rebuild_items i WHERE i.job_id=thumbnail_rebuild_jobs.id AND i.status='cancelled'),updated_at=?,error='服务器重启，缩略图重建已自动排队恢复' WHERE status IN ('queued','running')")
        .bind(now())
        .execute(pool)
        .await?;
    sqlx::query("UPDATE s3_cleanup_items SET status='queued',error='服务器重启，删除任务已安全中断' WHERE status='deleting'")
        .execute(pool)
        .await?;
    sqlx::query("UPDATE s3_cleanup_jobs SET status='interrupted',updated_at=?,error='服务器重启，S3 扫描或清理已安全中断，可手动继续' WHERE status='running'")
        .bind(now())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM admin_sessions WHERE expires_at<=?")
        .bind(now())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM admin_sessions WHERE administrator_id=1 AND token_hash NOT IN (SELECT token_hash FROM admin_sessions WHERE administrator_id=1 ORDER BY created_at DESC,rowid DESC LIMIT 16)")
        .execute(pool)
        .await?;
    Ok(())
}

async fn apply_web_cache_policy(request: axum::extract::Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let mut response = next.run(request).await;

    if path == "/api" || path.starts_with("/api/") {
        return response;
    }

    if path.starts_with("/_nuxt/") && response.status().is_success() {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    } else if response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"))
    {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        );
        response
            .headers_mut()
            .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
        response
            .headers_mut()
            .insert(header::EXPIRES, HeaderValue::from_static("0"));
    }

    response
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=info".into()))
        .init();
    let config = Config::from_env()?;
    if let Some(parent) =
        database_path(&config.database_url).and_then(|path| path.parent().map(Path::to_path_buf))
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::create_dir_all(&config.thumbnail_cache_dir).await?;
    let db = SqlitePoolOptions::new()
        .max_connections(8)
        .after_connect(|connection, _metadata| {
            Box::pin(async move {
                sqlx::query("PRAGMA foreign_keys = ON")
                    .execute(&mut *connection)
                    .await?;
                sqlx::query("PRAGMA busy_timeout = 5000")
                    .execute(&mut *connection)
                    .await?;
                Ok(())
            })
        })
        .connect(&config.database_url)
        .await?;
    setup_database(&db).await?;
    album_downloads::setup(&db).await?;
    let master_key = load_or_create_master_key(&config.master_key_file).await?;
    let storage = StorageService::new(db.clone(), master_key, config.thumbnail_cache_dir.clone());
    let photo_graph_lock = Arc::new(tokio::sync::Mutex::new(()));
    let storage_mutation_gate = Arc::new(tokio::sync::RwLock::new(()));
    match recover_pending_blobs(&storage, &db, true).await {
        Ok(count) if count > 0 => info!(count, "recovered pending storage objects"),
        Ok(_) => {}
        Err(error) => warn!("pending storage recovery deferred: {error:#}"),
    }
    {
        let _storage_guard = storage.gate.write().await;
        match drain_photo_deletion_outbox(&storage, &db).await {
            Ok(result) if result.removed > 0 || !result.failures.is_empty() => info!(
                removed = result.removed,
                failures = result.failures.len(),
                "replayed photo deletion outbox"
            ),
            Ok(_) => {}
            Err(error) => warn!("photo deletion recovery deferred: {error:#}"),
        }
        match drain_source_deletion_outbox(&storage, &db, None).await {
            Ok(result) if result.removed > 0 || !result.failures.is_empty() => info!(
                removed = result.removed,
                failures = result.failures.len(),
                "replayed source deletion outbox"
            ),
            Ok(_) => {}
            Err(error) => warn!("source deletion recovery deferred: {error:#}"),
        }
    }
    let janitor_storage = storage.clone();
    let janitor_db = db.clone();
    let janitor_photo_graph_lock = photo_graph_lock.clone();
    let janitor_storage_mutation_gate = storage_mutation_gate.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = sqlx::query("DELETE FROM admin_sessions WHERE expires_at<=?")
                .bind(now())
                .execute(&janitor_db)
                .await
            {
                warn!("expired session cleanup failed: {error:#}");
            }
            match recover_pending_blobs(&janitor_storage, &janitor_db, false).await {
                Ok(count) if count > 0 => info!(count, "cleaned pending storage objects"),
                Ok(_) => {}
                Err(error) => warn!("pending storage cleanup retry failed: {error:#}"),
            }
            let Ok(_mutation_guard) = janitor_storage_mutation_gate.try_read() else {
                continue;
            };
            let _photo_graph_guard = janitor_photo_graph_lock.lock().await;
            let _storage_guard = janitor_storage.gate.write().await;
            match drain_photo_deletion_outbox(&janitor_storage, &janitor_db).await {
                Ok(result) if result.removed > 0 || !result.failures.is_empty() => info!(
                    removed = result.removed,
                    failures = result.failures.len(),
                    "retried photo deletion outbox"
                ),
                Ok(_) => {}
                Err(error) => warn!("photo deletion retry failed: {error:#}"),
            }
            match drain_source_deletion_outbox(&janitor_storage, &janitor_db, None).await {
                Ok(result) if result.removed > 0 || !result.failures.is_empty() => info!(
                    removed = result.removed,
                    failures = result.failures.len(),
                    "retried source deletion outbox"
                ),
                Ok(_) => {}
                Err(error) => warn!("source deletion retry failed: {error:#}"),
            }
        }
    });
    let thumbnail_workers = automatic_thumbnail_worker_count();
    let state = AppState {
        db,
        storage,
        secure_cookies: config.secure_cookies,
        trust_proxy_headers: config.trust_proxy_headers,
        workers: config.workers,
        jobs: Arc::new(DashMap::new()),
        storage_tasks: Arc::new(DashMap::new()),
        thumbnail_tasks: Arc::new(DashMap::new()),
        s3_cleanup_tasks: Arc::new(DashMap::new()),
        storage_mutation_gate,
        conversion_slots: Arc::new(tokio::sync::Semaphore::new(config.workers)),
        export_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(DEFAULT_UPLOAD_CONCURRENCY)),
        thumbnail_slots: Arc::new(tokio::sync::Semaphore::new(thumbnail_workers)),
        thumbnail_workers,
        password_hash_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        photo_graph_lock,
        downloads: Arc::new(album_downloads::Service::new(
            config.thumbnail_cache_dir.with_file_name("album-downloads"),
        )),
    };
    album_downloads::start(state.clone()).await?;
    // A persisted administrator rebuild takes precedence over the opportunistic startup backfill.
    // Its phase and item states make a restart safe even if it happened while clearing the cache.
    if !recover_thumbnail_rebuild(state.clone()).await {
        spawn_thumbnail_backfill(state.clone());
    }
    let api = Router::new()
        .merge(album_downloads::routes())
        .route("/api/auth/status", get(auth_status))
        .route(
            "/api/auth/register",
            post(register_admin).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/auth/login",
            post(login_admin).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/auth/logout",
            post(logout_admin).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/settings/storage",
            get(get_storage_settings).put(save_storage_settings),
        )
        .route("/api/settings/storage/test", post(test_storage_settings))
        .route(
            "/api/storage-migrations",
            get(list_storage_migrations).post(start_storage_migration),
        )
        .route(
            "/api/storage-migrations/{job_id}/resume",
            post(resume_storage_migration),
        )
        .route(
            "/api/storage-migrations/{job_id}/cancel",
            post(cancel_storage_task),
        )
        .route(
            "/api/storage-migrations/{job_id}/cleanup",
            post(cleanup_old_storage),
        )
        .route(
            "/api/storage-migrations/{job_id}/retain",
            post(retain_old_storage),
        )
        .route("/api/s3-cleanups/latest", get(latest_s3_cleanup))
        .route("/api/s3-cleanups/scan", post(start_s3_cleanup_scan))
        .route(
            "/api/s3-cleanups/{job_id}/delete",
            post(start_s3_cleanup_delete),
        )
        .route("/api/s3-cleanups/{job_id}/cancel", post(cancel_s3_cleanup))
        .route("/api/s3-cleanups/{job_id}/resume", post(resume_s3_cleanup))
        .route(
            "/api/settings/site",
            get(get_site_settings).put(save_site_settings),
        )
        .route("/api/albums", get(list_albums).post(create_album))
        .route("/api/albums/order", post(reorder_albums))
        .route("/api/albums/export", get(export_albums))
        .route(
            "/api/albums/{album_id}/cover",
            post(album_covers::upload)
                .layer(DefaultBodyLimit::disable())
                .put(album_covers::select)
                .delete(album_covers::reset),
        )
        .route(
            "/api/albums/{album_id}/cover/{version}",
            get(album_covers::serve),
        )
        .route(
            "/api/albums/{album_id}",
            get(album_detail).patch(patch_album).delete(delete_album),
        )
        .route(
            "/api/albums/{album_id}/photos",
            get(album_photos)
                .post(upload_photos)
                .layer(DefaultBodyLimit::disable()),
        )
        .route("/api/photos", get(list_photos))
        .route("/api/photos/export", post(export_photos))
        .route("/api/photos/delete", post(delete_photos))
        .route(
            "/api/photos/{photo_id}",
            get(photo_detail).delete(delete_photo),
        )
        .route("/api/photos/{photo_id}/file", get(photo_file))
        .route("/api/photos/{photo_id}/thumbnail", get(photo_thumbnail))
        .route("/api/photos/{photo_id}/preview", get(photo_preview))
        .route("/api/photos/{photo_id}/high", get(photo_high))
        .route("/api/photos/{photo_id}/render", get(photo_render))
        .route(
            "/api/thumbnails/rebuilds/latest",
            get(latest_thumbnail_rebuild),
        )
        .route("/api/thumbnails/rebuilds", post(start_thumbnail_rebuild))
        .route(
            "/api/thumbnails/rebuilds/{job_id}/cancel",
            post(cancel_thumbnail_rebuild),
        )
        .route(
            "/api/thumbnails/rebuilds/{job_id}/resume",
            post(resume_thumbnail_rebuild),
        )
        .route(
            "/api/conversions",
            get(list_conversions).post(start_conversion),
        )
        .route("/api/conversions/{job_id}", get(get_conversion))
        .route("/api/conversions/{job_id}/cancel", post(cancel_conversion))
        .route(
            "/api/conversions/{job_id}/delete-sources",
            delete(confirm_delete_sources),
        )
        .route("/api", any(api_not_found))
        .route("/api/{*path}", any(api_not_found));
    let web_dir = PathBuf::from(&config.web_dir);
    let index = web_dir.join("index.html");
    let app = Router::new()
        .merge(api)
        .nest_service("/_nuxt", ServeDir::new(web_dir.join("_nuxt")))
        .fallback_service(ServeDir::new(&config.web_dir).fallback(ServeFile::new(index)))
        .layer(middleware::from_fn(apply_web_cache_policy))
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let bind_addr = env::var("CF_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("ChronoFrame listening on http://{bind_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) async fn test_state() -> AppState {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        setup_database(&db).await.unwrap();
        album_downloads::setup(&db).await.unwrap();
        let thumbnail_cache_dir =
            std::env::temp_dir().join(format!("chronoframe-test-thumbnails-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&thumbnail_cache_dir)
            .await
            .unwrap();
        AppState {
            downloads: Arc::new(album_downloads::Service::new(
                thumbnail_cache_dir.join("album-downloads"),
            )),
            storage: StorageService::new(db.clone(), [7u8; 32], thumbnail_cache_dir),
            db,
            secure_cookies: Some(false),
            trust_proxy_headers: false,
            workers: 2,
            jobs: Arc::new(DashMap::new()),
            storage_tasks: Arc::new(DashMap::new()),
            thumbnail_tasks: Arc::new(DashMap::new()),
            s3_cleanup_tasks: Arc::new(DashMap::new()),
            storage_mutation_gate: Arc::new(tokio::sync::RwLock::new(())),
            conversion_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            export_slots: Arc::new(tokio::sync::Semaphore::new(1)),
            upload_slots: Arc::new(tokio::sync::Semaphore::new(DEFAULT_UPLOAD_CONCURRENCY)),
            thumbnail_slots: Arc::new(tokio::sync::Semaphore::new(1)),
            thumbnail_workers: 1,
            password_hash_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            photo_graph_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn png_fixture(width: u32, height: u32) -> Vec<u8> {
        let image = image::DynamicImage::new_rgb8(width, height);
        let mut output = Cursor::new(Vec::new());
        image.write_to(&mut output, ImageFormat::Png).unwrap();
        output.into_inner()
    }

    #[tokio::test]
    async fn upload_queue_allows_exactly_seven_concurrent_requests() {
        let state = test_state().await;
        let permits = state
            .upload_slots
            .clone()
            .try_acquire_many_owned(DEFAULT_UPLOAD_CONCURRENCY as u32)
            .unwrap();
        assert_eq!(state.upload_slots.available_permits(), 0);
        assert!(state.upload_slots.clone().try_acquire_owned().is_err());
        drop(permits);
        assert_eq!(
            state.upload_slots.available_permits(),
            DEFAULT_UPLOAD_CONCURRENCY
        );
    }

    #[test]
    fn thumbnail_is_png_and_limits_the_longest_edge() {
        let thumbnail = encode_thumbnail(&png_fixture(1600, 800), 720).unwrap();
        assert_eq!(image::guess_format(&thumbnail).unwrap(), ImageFormat::Png);
        let decoded = image::load_from_memory(&thumbnail).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (720, 360));
    }

    #[test]
    fn thumbnail_does_not_enlarge_small_images() {
        let thumbnail = encode_thumbnail(&png_fixture(120, 80), 720).unwrap();
        let decoded = image::load_from_memory(&thumbnail).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (120, 80));
    }

    #[test]
    fn s3_cleanup_only_selects_old_unreferenced_managed_objects() {
        let mut protected = HashSet::new();
        protected.insert("albums/album/original/live.webp".to_string());
        let old = now() - S3_ORPHAN_GRACE_SECONDS - 1;
        let recent = now() - S3_ORPHAN_GRACE_SECONDS + 1;
        let object = |logical_key: &str, last_modified: i64| S3ManagedObject {
            physical_key: format!("chronoframe/{logical_key}"),
            logical_key: logical_key.to_string(),
            byte_size: 42,
            last_modified,
        };
        assert!(!s3_cleanup_candidate(
            &object("albums/album/original/live.webp", old),
            &protected,
            now() - S3_ORPHAN_GRACE_SECONDS,
        ));
        assert!(!s3_cleanup_candidate(
            &object("albums/album/original/recent.webp", recent),
            &protected,
            now() - S3_ORPHAN_GRACE_SECONDS,
        ));
        assert!(s3_cleanup_candidate(
            &object("albums/album/original/orphan.webp", old),
            &protected,
            now() - S3_ORPHAN_GRACE_SECONDS,
        ));
    }

    #[tokio::test]
    async fn setup_database_makes_interrupted_s3_cleanup_resumable() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        setup_database(&pool).await.unwrap();
        sqlx::query("INSERT INTO s3_cleanup_jobs(id,status,phase,total,worker_count,location_key,managed_prefix,created_at,updated_at) VALUES('cleanup','running','deleting',1,8,'s3:test','chronoframe/albums/',1,1); INSERT INTO s3_cleanup_items(id,job_id,object_key,logical_key,byte_size,last_modified,status) VALUES('item','cleanup','chronoframe/albums/a/original/x.webp','albums/a/original/x.webp',10,1,'deleting');")
            .execute(&pool)
            .await
            .unwrap();

        setup_database(&pool).await.unwrap();

        let status: String =
            sqlx::query_scalar("SELECT status FROM s3_cleanup_jobs WHERE id='cleanup'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let item_status: String =
            sqlx::query_scalar("SELECT status FROM s3_cleanup_items WHERE id='item'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "interrupted");
        assert_eq!(item_status, "queued");
    }

    #[tokio::test]
    async fn thumbnail_cache_persists_and_is_removed_with_the_photo() {
        let state = test_state().await;
        let photo_id = Uuid::new_v4().to_string();
        let expected = encode_thumbnail(&png_fixture(64, 48), 720).unwrap();
        let legacy_paths = state.storage.legacy_thumbnail_paths(&photo_id).unwrap();
        for legacy_path in &legacy_paths {
            tokio::fs::write(legacy_path, b"legacy-thumbnail")
                .await
                .unwrap();
        }
        state
            .storage
            .cache_thumbnail(&photo_id, &expected)
            .await
            .unwrap();
        let derivatives = encode_all_derivatives(&png_fixture(640, 480)).unwrap();
        for (derivative, data) in derivatives {
            state
                .storage
                .cache_derivative(&photo_id, derivative, &data)
                .await
                .unwrap();
        }
        assert_eq!(
            state
                .storage
                .thumbnail_path(&photo_id)
                .unwrap()
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("png")
        );
        assert!(legacy_paths.iter().all(|path| !path.exists()));
        assert!(
            state
                .storage
                .cached_thumbnail(&photo_id)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            state
                .storage
                .cached_derivative(&photo_id, ImageDerivative::Preview)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            state
                .storage
                .cached_derivative(&photo_id, ImageDerivative::High)
                .await
                .unwrap()
                .is_some()
        );
        state.storage.remove_cached_thumbnail(&photo_id).await;
        assert_eq!(
            state.storage.cached_thumbnail(&photo_id).await.unwrap(),
            None
        );
        assert!(
            state
                .storage
                .cached_derivative(&photo_id, ImageDerivative::Preview)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .storage
                .cached_derivative(&photo_id, ImageDerivative::High)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn three_derivatives_have_expected_formats_dimensions_and_size_caps() {
        let source = png_fixture(3200, 1800);
        let derivatives = encode_all_derivatives(&source).unwrap();
        assert_eq!(derivatives.len(), 3);
        for (kind, data) in derivatives {
            match kind {
                ImageDerivative::Grid => {
                    assert_eq!(image::guess_format(&data).unwrap(), ImageFormat::Png);
                    let decoded = image::load_from_memory(&data).unwrap();
                    assert_eq!((decoded.width(), decoded.height()), (320, 180));
                }
                ImageDerivative::Preview => {
                    assert_eq!(image::guess_format(&data).unwrap(), ImageFormat::WebP);
                    assert!(data.len() <= VIEW_PREVIEW_MAX_BYTES);
                }
                ImageDerivative::High => {
                    assert_eq!(image::guess_format(&data).unwrap(), ImageFormat::WebP);
                    assert!(data.len() <= VIEW_HIGH_MAX_BYTES);
                }
            }
        }
    }

    #[test]
    fn converted_image_reports_its_actual_dimensions() {
        let converted = encode_image(&png_fixture(321, 123), "jpg").unwrap();
        assert_eq!((converted.width, converted.height), (321, 123));
        let decoded = image::load_from_memory(&converted.data).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (321, 123));
    }

    #[test]
    fn passwords_use_explicit_argon2id_parameters_and_verify() {
        let encoded = hash_password("a sufficiently long password".into()).unwrap();
        assert!(encoded.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(verify_password("a sufficiently long password".into(), encoded.clone()).unwrap());
        assert!(!verify_password("a different long password".into(), encoded).unwrap());
    }

    #[test]
    fn legacy_master_key_matches_the_previous_token_derivation() {
        let seed = "legacy-token-used-only-for-storage-migration";
        let expected = Sha256::digest(seed.as_bytes());
        assert_eq!(&legacy_storage_key(seed)[..], &expected[..]);
    }

    #[test]
    fn export_names_strip_paths_and_disambiguate_case_insensitively() {
        assert_eq!(
            sanitize_export_name("../../photo?.png", "photo"),
            "photo_.png"
        );
        assert_eq!(
            sanitize_export_name("..\\camera\\frame.jpg", "photo"),
            "frame.jpg"
        );
        assert_eq!(sanitize_export_name("...", "album"), "album");
        let mut used = HashSet::new();
        assert_eq!(
            unique_export_name("Photo.JPG".into(), &mut used),
            "Photo.JPG"
        );
        assert_eq!(
            unique_export_name("photo.jpg".into(), &mut used),
            "photo (2).jpg"
        );
    }

    #[test]
    fn site_settings_trim_public_values_and_reject_unsafe_urls() {
        let normalized = SiteSettings {
            title: "  My Gallery  ".into(),
            slogan: "  A slogan  ".into(),
            author: "  Owner  ".into(),
            avatar_url: "/avatar.png".into(),
            theme: " DARK ".into(),
        }
        .normalize()
        .unwrap();
        assert_eq!(normalized.title, "My Gallery");
        assert_eq!(normalized.slogan, "A slogan");
        assert_eq!(normalized.author, "Owner");
        assert_eq!(normalized.theme, "dark");
        assert!(
            SiteSettings {
                avatar_url: "javascript:alert(1)".into(),
                ..SiteSettings::defaults()
            }
            .normalize()
            .is_err()
        );
    }

    #[test]
    fn administrator_username_rejects_control_characters() {
        assert!(
            validate_credentials(CredentialsInput {
                username: "admin\nspoofed".into(),
                password: "a sufficiently long password".into(),
            })
            .is_err()
        );
    }

    #[tokio::test]
    async fn administrator_schema_allows_exactly_one_concurrent_winner() {
        let state = test_state().await;
        let first = sqlx::query("INSERT INTO administrators(id,username,password_hash,created_at) VALUES(1,?,?,?) ON CONFLICT(id) DO NOTHING")
            .bind("first")
            .bind("hash-one")
            .bind(now())
            .execute(&state.db);
        let second = sqlx::query("INSERT INTO administrators(id,username,password_hash,created_at) VALUES(1,?,?,?) ON CONFLICT(id) DO NOTHING")
            .bind("second")
            .bind("hash-two")
            .bind(now())
            .execute(&state.db);
        let (first, second) = tokio::join!(first, second);
        let affected = first.unwrap().rows_affected() + second.unwrap().rows_affected();
        assert_eq!(affected, 1);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM administrators")
                .fetch_one(&state.db)
                .await
                .unwrap(),
            1
        );
        assert!(
            sqlx::query("INSERT INTO administrators(id,username,password_hash,created_at) VALUES(2,'forbidden','hash',?)")
                .bind(now())
                .execute(&state.db)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn expired_session_is_rejected_and_removed() {
        let state = test_state().await;
        sqlx::query("INSERT INTO administrators(id,username,password_hash,created_at) VALUES(1,'admin','hash',?)")
            .bind(now())
            .execute(&state.db)
            .await
            .unwrap();
        let token = "expired-session-token";
        sqlx::query("INSERT INTO admin_sessions(token_hash,administrator_id,csrf_hash,created_at,expires_at) VALUES(?,1,?,?,?)")
            .bind(digest_secret(token))
            .bind(digest_secret("csrf"))
            .bind(now() - 10)
            .bind(now() - 1)
            .execute(&state.db)
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{SESSION_COOKIE}={token}")).unwrap(),
        );
        let error = match require_admin(&headers, &state, false).await {
            Ok(_) => panic!("expired session was accepted"),
            Err(error) => error,
        };
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .count(),
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM admin_sessions")
                .fetch_one(&state.db)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn csrf_rotation_uses_compare_and_swap() {
        let state = test_state().await;
        sqlx::query("INSERT INTO administrators(id,username,password_hash,created_at) VALUES(1,'admin','hash',?)")
            .bind(now())
            .execute(&state.db)
            .await
            .unwrap();
        let token_hash = digest_secret("active-session");
        let old_hash = digest_secret("old-csrf");
        let first_hash = digest_secret("first-csrf");
        let second_hash = digest_secret("second-csrf");
        sqlx::query("INSERT INTO admin_sessions(token_hash,administrator_id,csrf_hash,created_at,expires_at) VALUES(?,1,?,?,?)")
            .bind(&token_hash)
            .bind(&old_hash)
            .bind(now())
            .bind(now() + 60)
            .execute(&state.db)
            .await
            .unwrap();

        assert!(
            rotate_csrf_if_current(&state.db, &token_hash, &old_hash, &first_hash)
                .await
                .unwrap()
        );
        assert!(
            !rotate_csrf_if_current(&state.db, &token_hash, &old_hash, &second_hash)
                .await
                .unwrap()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT csrf_hash FROM admin_sessions WHERE token_hash=?",
            )
            .bind(token_hash)
            .fetch_one(&state.db)
            .await
            .unwrap(),
            first_hash
        );
    }

    #[tokio::test]
    async fn sessions_are_capped_at_the_sixteen_most_recent() {
        let state = test_state().await;
        sqlx::query("INSERT INTO administrators(id,username,password_hash,created_at) VALUES(1,'admin','hash',?)")
            .bind(now())
            .execute(&state.db)
            .await
            .unwrap();
        let mut newest_token = String::new();
        for _ in 0..20 {
            let (token, _) = create_session(&state).await.unwrap();
            newest_token = token;
        }

        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM admin_sessions")
                .fetch_one(&state.db)
                .await
                .unwrap(),
            16
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM admin_sessions WHERE token_hash=?",)
                .bind(digest_secret(&newest_token))
                .fetch_one(&state.db)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn proxy_https_headers_are_ignored_unless_explicitly_trusted() {
        let state = test_state().await;
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert!(!request_is_https(&headers, &state));

        let mut trusted_state = state.clone();
        trusted_state.trust_proxy_headers = true;
        assert!(request_is_https(&headers, &trusted_state));
    }

    #[test]
    fn requested_with_allows_proxy_domain_and_ip_origins() {
        for origin in [
            "https://gallery.example.com",
            "http://192.0.2.10:8188",
            "https://another-proxy.example:8443",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert("x-requested-with", HeaderValue::from_static(REQUESTED_WITH));
            headers.insert(header::ORIGIN, HeaderValue::from_str(origin).unwrap());
            headers.insert(header::HOST, HeaderValue::from_static("chronoframe:8080"));
            assert!(require_requested_with(&headers).is_ok(), "origin: {origin}");
        }
    }

    #[test]
    fn album_dates_are_strict_real_calendar_dates() {
        for value in ["0001-01-01", "2000-02-29", "2024-02-29", "9999-12-31"] {
            assert!(is_valid_album_date(value), "expected valid date: {value}");
            assert_eq!(
                normalize_album_date(Some(value.into()), "日期")
                    .unwrap()
                    .as_deref(),
                Some(value)
            );
        }
        for value in [
            "0000-01-01",
            "1900-02-29",
            "2023-02-29",
            "2024-00-01",
            "2024-13-01",
            "2024-04-31",
            "2024-2-29",
            "2024-02-9",
            "2024-02-29T00:00:00Z",
            " 2024-02-29",
            "2024-02-29 ",
        ] {
            assert!(
                !is_valid_album_date(value),
                "expected invalid date: {value}"
            );
            assert!(normalize_album_date(Some(value.into()), "日期").is_err());
        }
        assert_eq!(normalize_album_date(None, "日期").unwrap(), None);
        assert_eq!(
            normalize_album_date(Some(" \t\r\n ".into()), "日期").unwrap(),
            None
        );
    }

    #[test]
    fn photo_date_range_requires_an_ordered_pair() {
        let start = Some("2024-01-01".to_string());
        let same = Some("2024-01-01".to_string());
        let end = Some("2024-12-31".to_string());
        assert!(validate_photo_date_range(&None, &None).is_ok());
        assert!(validate_photo_date_range(&start, &same).is_ok());
        assert!(validate_photo_date_range(&start, &end).is_ok());
        assert!(validate_photo_date_range(&start, &None).is_err());
        assert!(validate_photo_date_range(&None, &end).is_err());
        assert!(validate_photo_date_range(&end, &start).is_err());
    }

    #[test]
    fn album_description_is_trimmed_and_bounded() {
        assert_eq!(normalize_album_description(None).unwrap(), "");
        assert_eq!(
            normalize_album_description(Some("  一段简介\n".into())).unwrap(),
            "一段简介"
        );
        assert!(normalize_album_description(Some("介".repeat(1000))).is_ok());
        assert!(normalize_album_description(Some("介".repeat(1001))).is_err());
    }

    #[test]
    fn album_name_is_trimmed_and_bounded() {
        assert_eq!(
            normalize_album_name(Some("  夏日旅行\n".into())).unwrap(),
            "夏日旅行"
        );
        assert!(normalize_album_name(None).is_err());
        assert!(normalize_album_name(Some(" \t ".into())).is_err());
        assert!(normalize_album_name(Some("相".repeat(100))).is_ok());
        assert!(normalize_album_name(Some("相".repeat(101))).is_err());
    }

    #[tokio::test]
    async fn single_photo_metadata_returns_only_the_requested_photo() {
        let state = test_state().await;
        sqlx::query("INSERT INTO albums(id,name,created_at) VALUES('album','Album',1)")
            .execute(&state.db)
            .await
            .unwrap();
        for id in ["first", "second"] {
            sqlx::query("INSERT INTO photos(id,album_id,original_name,storage_key,format,content_type,byte_size,created_at) VALUES(?,'album',?,?,'png','image/png',12,1)")
                .bind(id).bind(id).bind(format!("albums/album/{id}.png"))
                .execute(&state.db).await.unwrap();
        }
        let Json(photo) = photo_detail(State(state.clone()), AxumPath("second".into()))
            .await
            .unwrap();
        assert_eq!(photo.id, "second");
        assert_eq!(photo.album_id, "album");
        let error = photo_detail(State(state), AxumPath("missing".into()))
            .await
            .err()
            .unwrap();
        assert_eq!(error.status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn album_can_be_renamed_and_empty_album_can_be_deleted() {
        let state = test_state().await;
        sqlx::query("INSERT INTO administrators(id,username,password_hash,created_at) VALUES(1,'admin','hash',?)")
            .bind(now())
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO albums(id,name,description,created_at,position) VALUES('first','First','',1,0),('second','Second','',2,1)")
            .execute(&state.db)
            .await
            .unwrap();
        let (token, csrf) = create_session(&state).await.unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{SESSION_COOKIE}={token}; {CSRF_COOKIE}={csrf}"))
                .unwrap(),
        );
        headers.insert("x-csrf-token", HeaderValue::from_str(&csrf).unwrap());

        let Json(renamed) = patch_album(
            State(state.clone()),
            AxumPath("first".into()),
            headers.clone(),
            Json(AlbumPatch {
                name: PatchString::Present(Some("  Renamed album  ".into())),
                description: PatchString::Missing,
                display_created_date: PatchString::Missing,
                photo_date_start: PatchString::Missing,
                photo_date_end: PatchString::Missing,
            }),
        )
        .await
        .unwrap();
        assert_eq!(renamed.name, "Renamed album");

        let Json(result) = delete_album(State(state.clone()), AxumPath("first".into()), headers)
            .await
            .unwrap();
        assert_eq!(result["deleted"], true);
        assert_eq!(result["photosDeleted"], 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM albums WHERE id='first'")
                .fetch_one(&state.db)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT position FROM albums WHERE id='second'")
                .fetch_one(&state.db)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn setup_database_adds_album_metadata_without_losing_rows() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE albums (id TEXT PRIMARY KEY,name TEXT NOT NULL,created_at INTEGER NOT NULL); INSERT INTO albums(id,name,created_at) VALUES('legacy-album','Legacy Album',12345),('newer-album','Newer Album',23456);")
            .execute(&pool)
            .await
            .unwrap();

        setup_database(&pool).await.unwrap();

        let legacy = sqlx::query("SELECT name,description,created_at,display_created_date,photo_date_start,photo_date_end,position FROM albums WHERE id='legacy-album'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(legacy.get::<String, _>("name"), "Legacy Album");
        assert_eq!(legacy.get::<String, _>("description"), "");
        assert_eq!(legacy.get::<i64, _>("created_at"), 12345);
        assert_eq!(legacy.get::<i64, _>("position"), 1);
        assert_eq!(
            legacy.get::<Option<String>, _>("display_created_date"),
            None
        );
        assert_eq!(legacy.get::<Option<String>, _>("photo_date_start"), None);
        assert_eq!(legacy.get::<Option<String>, _>("photo_date_end"), None);

        let newer_position: i64 =
            sqlx::query_scalar("SELECT position FROM albums WHERE id='newer-album'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(newer_position, 0);

        sqlx::query("UPDATE albums SET description='Legacy description',display_created_date='2020-01-02',photo_date_start='2019-01-01',photo_date_end='2020-01-01' WHERE id='legacy-album'")
            .execute(&pool)
            .await
            .unwrap();
        setup_database(&pool).await.unwrap();

        let migrated = sqlx::query("SELECT name,description,created_at,display_created_date,photo_date_start,photo_date_end,position FROM albums WHERE id='legacy-album'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(migrated.get::<String, _>("name"), "Legacy Album");
        assert_eq!(
            migrated.get::<String, _>("description"),
            "Legacy description"
        );
        assert_eq!(migrated.get::<i64, _>("created_at"), 12345);
        assert_eq!(migrated.get::<i64, _>("position"), 1);
        assert_eq!(
            migrated
                .get::<Option<String>, _>("display_created_date")
                .as_deref(),
            Some("2020-01-02")
        );
        assert_eq!(
            migrated
                .get::<Option<String>, _>("photo_date_start")
                .as_deref(),
            Some("2019-01-01")
        );
        assert_eq!(
            migrated
                .get::<Option<String>, _>("photo_date_end")
                .as_deref(),
            Some("2020-01-01")
        );
        let metadata_columns = sqlx::query("PRAGMA table_info(albums)")
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .filter(|column| {
                matches!(
                    column.get::<String, _>("name").as_str(),
                    "description"
                        | "display_created_date"
                        | "photo_date_start"
                        | "photo_date_end"
                        | "position"
                )
            })
            .count();
        assert_eq!(metadata_columns, 5);
    }

    #[tokio::test]
    async fn setup_database_adds_missing_photo_dimensions_without_losing_rows() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE albums (id TEXT PRIMARY KEY,name TEXT NOT NULL,created_at INTEGER NOT NULL); CREATE TABLE photos (id TEXT PRIMARY KEY,album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,original_name TEXT NOT NULL,storage_key TEXT NOT NULL UNIQUE,format TEXT NOT NULL CHECK(format IN ('png','jpg','webp')),content_type TEXT NOT NULL,byte_size INTEGER NOT NULL,created_at INTEGER NOT NULL); INSERT INTO albums(id,name,created_at) VALUES('album','Album',1); INSERT INTO photos(id,album_id,original_name,storage_key,format,content_type,byte_size,created_at) VALUES('photo','album','old.png','old.png','png','image/png',10,1);")
            .execute(&pool)
            .await
            .unwrap();

        setup_database(&pool).await.unwrap();

        let row = sqlx::query("SELECT width,height,original_name FROM photos WHERE id='photo'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<i64, _>("width"), 0);
        assert_eq!(row.get::<i64, _>("height"), 0);
        assert_eq!(row.get::<String, _>("original_name"), "old.png");

        setup_database(&pool).await.unwrap();
        let dimensions = sqlx::query("PRAGMA table_info(photos)")
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .filter(|column| matches!(column.get::<String, _>("name").as_str(), "width" | "height"))
            .count();
        assert_eq!(dimensions, 2);
    }
}
