use std::{
    collections::{HashMap, HashSet},
    env,
    io::{Cursor, ErrorKind},
    path::{Path, PathBuf},
    sync::Arc,
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
    body::Bytes,
    extract::{
        DefaultBodyLimit, Multipart, Path as AxumPath, Query, State,
        rejection::{BytesRejection, JsonRejection},
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use dashmap::DashMap;
use futures_util::stream::{self, StreamExt};
use image::ImageFormat;
use rand::{RngCore, rngs::OsRng};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, sqlite::SqlitePoolOptions};
use subtle::ConstantTimeEq;
use tokio::{io::AsyncWriteExt, time::timeout};
use tokio_util::sync::CancellationToken;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    storage: StorageService,
    secure_cookies: Option<bool>,
    trust_proxy_headers: bool,
    workers: usize,
    jobs: Arc<DashMap<String, CancellationToken>>,
    conversion_slots: Arc<tokio::sync::Semaphore>,
    upload_slots: Arc<tokio::sync::Semaphore>,
    thumbnail_slots: Arc<tokio::sync::Semaphore>,
    password_hash_slots: Arc<tokio::sync::Semaphore>,
    photo_graph_lock: Arc<tokio::sync::Mutex<()>>,
}

const STORAGE_IO_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_UPLOAD_BATCH_BYTES: usize = 384 * 1024 * 1024;
const MAX_UPLOAD_FILES: usize = 128;
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
        Ok(Self {
            database_url,
            master_key_file: env::var("CF_MASTER_KEY_FILE")
                .map(PathBuf::from)
                .unwrap_or(default_master_key),
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
            workers: get("CF_CONVERSION_WORKERS", "4")
                .parse::<usize>()
                .unwrap_or(4)
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
    cache: SharedStoreCache,
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

#[derive(Clone)]
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

impl StorageService {
    fn new(db: SqlitePool, encryption_key: [u8; 32]) -> Self {
        Self {
            db,
            encryption_key,
            gate: Arc::new(tokio::sync::RwLock::new(())),
            cache: Arc::new(tokio::sync::RwLock::new(None)),
        }
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
                "SELECT (SELECT COUNT(*) FROM photos) + (SELECT COUNT(*) FROM pending_blobs)",
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
    created_at: i64,
    display_created_date: Option<String>,
    photo_date_start: Option<String>,
    photo_date_end: Option<String>,
    photo_count: i64,
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

fn album_from(row: &sqlx::sqlite::SqliteRow) -> Album {
    Album {
        id: row.get("id"),
        name: row.get("name"),
        created_at: row.get("created_at"),
        display_created_date: row.get("display_created_date"),
        photo_date_start: row.get("photo_date_start"),
        photo_date_end: row.get("photo_date_end"),
        photo_count: row.get("photo_count"),
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
    let rows = if let Some(job_id) = only_job {
        sqlx::query("SELECT job_id,source_photo_id,source_key,target_key,target_format,target_size FROM source_deletion_outbox WHERE job_id=? ORDER BY created_at,source_photo_id").bind(job_id).fetch_all(db).await?
    } else {
        sqlx::query("SELECT job_id,source_photo_id,source_key,target_key,target_format,target_size FROM source_deletion_outbox ORDER BY created_at,source_photo_id").fetch_all(db).await?
    };
    let store = if rows.is_empty() {
        None
    } else {
        Some(storage.store().await?)
    };
    let mut result = SourceDeletionDrain::default();
    for row in rows {
        let job_id: String = row.get("job_id");
        let source_photo_id: String = row.get("source_photo_id");
        let source_key: String = row.get("source_key");
        let target_key: String = row.get("target_key");
        let target_format: String = row.get("target_format");
        let target_size: i64 = row.get("target_size");
        let store = store.as_ref().expect("outbox rows require a store");
        if let Err(error) =
            verify_target_blob(store, &target_key, &target_format, target_size).await
        {
            result.failures.push(serde_json::json!({"photoId": source_photo_id, "error": format!("目标图验证失败：{error:#}")}));
            continue;
        }
        if let Err(error) = store.delete(&source_key).await {
            result
                .failures
                .push(serde_json::json!({"photoId": source_photo_id, "error": error.to_string()}));
            continue;
        }
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
        result.removed += 1;
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

#[derive(Deserialize)]
struct NewAlbum {
    name: String,
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
struct AlbumDatePatch {
    #[serde(default)]
    display_created_date: PatchString,
    #[serde(default)]
    photo_date_start: PatchString,
    #[serde(default)]
    photo_date_end: PatchString,
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
    let rows = sqlx::query("SELECT a.id,a.name,a.created_at,a.display_created_date,a.photo_date_start,a.photo_date_end,COUNT(p.id) photo_count FROM albums a LEFT JOIN photos p ON p.album_id=a.id GROUP BY a.id ORDER BY a.created_at DESC").fetch_all(&state.db).await.map_err(AppError::internal)?;
    Ok(Json(rows.iter().map(album_from).collect()))
}
async fn album_detail(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
) -> ApiResult<Json<AlbumDetail>> {
    let row = sqlx::query("SELECT a.id,a.name,a.created_at,a.display_created_date,a.photo_date_start,a.photo_date_end,COUNT(p.id) photo_count FROM albums a LEFT JOIN photos p ON p.album_id=a.id WHERE a.id=? GROUP BY a.id")
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
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::bad("相簿名不能为空且不得超过 100 个字符"));
    }
    let album = Album {
        id: Uuid::new_v4().to_string(),
        name: name.into(),
        created_at: now(),
        display_created_date: None,
        photo_date_start: None,
        photo_date_end: None,
        photo_count: 0,
    };
    sqlx::query("INSERT INTO albums(id,name,created_at) VALUES(?,?,?)")
        .bind(&album.id)
        .bind(&album.name)
        .bind(album.created_at)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?;
    Ok((StatusCode::CREATED, Json(album)))
}

async fn patch_album_dates(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
    headers: HeaderMap,
    Json(input): Json<AlbumDatePatch>,
) -> ApiResult<Json<Album>> {
    require_admin(&headers, &state, true).await?;

    let display_created_date = match input.display_created_date {
        PatchString::Missing => None,
        PatchString::Present(value) => Some(normalize_album_date(value, "展示创建日期")?),
    };
    let photo_date_range = match (input.photo_date_start, input.photo_date_end) {
        (PatchString::Missing, PatchString::Missing) => None,
        (PatchString::Present(start), PatchString::Present(end)) => {
            let start = normalize_album_date(start, "图片开始日期")?;
            let end = normalize_album_date(end, "图片结束日期")?;
            validate_photo_date_range(&start, &end)?;
            Some((start, end))
        }
        _ => {
            return Err(AppError::bad(
                "图片开始日期和结束日期必须同时设置或同时清除",
            ));
        }
    };

    let result = match (display_created_date, photo_date_range) {
        (Some(display_created_date), Some((photo_date_start, photo_date_end))) => {
            sqlx::query("UPDATE albums SET display_created_date=?,photo_date_start=?,photo_date_end=? WHERE id=?")
                .bind(display_created_date)
                .bind(photo_date_start)
                .bind(photo_date_end)
                .bind(&album_id)
                .execute(&state.db)
                .await
                .map_err(AppError::internal)?
        }
        (Some(display_created_date), None) => sqlx::query(
            "UPDATE albums SET display_created_date=? WHERE id=?",
        )
        .bind(display_created_date)
        .bind(&album_id)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?,
        (None, Some((photo_date_start, photo_date_end))) => sqlx::query(
            "UPDATE albums SET photo_date_start=?,photo_date_end=? WHERE id=?",
        )
        .bind(photo_date_start)
        .bind(photo_date_end)
        .bind(&album_id)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?,
        (None, None) => return Err(AppError::bad("至少需要提供一个日期字段")),
    };
    if result.rows_affected() == 0 {
        return Err(AppError {
            status: StatusCode::NOT_FOUND,
            message: "相簿不存在".into(),
            clear_auth_cookies: None,
        });
    }

    let row = sqlx::query("SELECT a.id,a.name,a.created_at,a.display_created_date,a.photo_date_start,a.photo_date_end,COUNT(p.id) photo_count FROM albums a LEFT JOIN photos p ON p.album_id=a.id WHERE a.id=? GROUP BY a.id")
        .bind(&album_id)
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(album_from(&row)))
}
async fn album_photos(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
) -> ApiResult<Json<Vec<Photo>>> {
    let rows = sqlx::query("SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos WHERE album_id=? ORDER BY created_at DESC,id DESC").bind(album_id).fetch_all(&state.db).await.map_err(AppError::internal)?;
    Ok(Json(rows.iter().map(photo_from).collect()))
}
async fn list_photos(State(state): State<AppState>) -> ApiResult<Json<Vec<Photo>>> {
    let rows = sqlx::query("SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos ORDER BY created_at DESC,id DESC")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(rows.iter().map(photo_from).collect()))
}
async fn upload_photos(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<Json<Vec<Photo>>> {
    require_admin(&headers, &state, true).await?;
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
    // Validate the complete multipart batch before writing anything. A bad file therefore cannot leave a half-visible upload.
    let mut prepared: Vec<(Photo, Vec<u8>)> = vec![];
    let mut batch_bytes = 0usize;
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
        if prepared.len() >= MAX_UPLOAD_FILES {
            return Err(AppError::bad(format!(
                "单次最多上传 {MAX_UPLOAD_FILES} 张图片"
            )));
        }
        if bytes.len() > 100 * 1024 * 1024 {
            return Err(AppError::bad(format!("{filename} 超过 100MB 限制")));
        }
        batch_bytes = batch_bytes.saturating_add(bytes.len());
        if batch_bytes > MAX_UPLOAD_BATCH_BYTES {
            return Err(AppError::bad("单次上传总大小不得超过 384MB"));
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
    Ok(Json(prepared.into_iter().map(|(photo, _)| photo).collect()))
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
            (header::CACHE_CONTROL, "private, max-age=3600".to_string()),
        ],
        data,
    )
        .into_response())
}

async fn photo_thumbnail(
    State(state): State<AppState>,
    AxumPath(photo_id): AxumPath<String>,
) -> ApiResult<Response> {
    let _permit = state
        .thumbnail_slots
        .acquire()
        .await
        .map_err(|_| AppError::bad("缩略图工作池已关闭"))?;
    let storage_key: String = sqlx::query_scalar("SELECT storage_key FROM photos WHERE id=?")
        .bind(photo_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError {
            status: StatusCode::NOT_FOUND,
            message: "图片不存在".into(),
            clear_auth_cookies: None,
        })?;
    let data = {
        let _storage_guard = state.storage.gate.read().await;
        state
            .storage
            .store()
            .await
            .map_err(AppError::internal)?
            .get(&storage_key)
            .await
            .map_err(AppError::internal)?
    };
    let thumbnail = tokio::task::spawn_blocking(move || encode_thumbnail(&data, 720))
        .await
        .map_err(AppError::internal)?
        .map_err(AppError::internal)?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/webp"),
            (header::CACHE_CONTROL, "private, max-age=86400"),
        ],
        thumbnail,
    )
        .into_response())
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
    let rows = sqlx::query("SELECT id,status,target_format,total,completed,succeeded,failed,cancelled,created_at,updated_at,sources_deleted_at FROM conversion_jobs ORDER BY created_at DESC LIMIT 100").fetch_all(&state.db).await.map_err(AppError::internal)?;
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
    let row = sqlx::query("SELECT id,status,target_format,total,completed,succeeded,failed,cancelled,created_at,updated_at,sources_deleted_at FROM conversion_jobs WHERE id=?").bind(&job_id).fetch_optional(&state.db).await.map_err(AppError::internal)?.ok_or_else(|| AppError { status: StatusCode::NOT_FOUND, message: "转换任务不存在".into(), clear_auth_cookies: None })?;
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
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, true).await?;
    let _photo_graph_guard = state.photo_graph_lock.lock().await;
    let _storage_guard = state.storage.gate.read().await;
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
    let storage = state.storage.store().await.map_err(AppError::internal)?;
    let mut prepared = vec![];
    let mut validation_failures = vec![];
    for row in rows {
        let id: String = row.get("id");
        let source_key: String = row.get("storage_key");
        let target_key: Option<String> = row.get("target_key");
        let target_format: Option<String> = row.get("target_format");
        let target_size: Option<i64> = row.get("target_size");
        let verified = async {
            let target_key = target_key.ok_or_else(|| anyhow!("转换图记录已不存在"))?;
            let target_format = target_format.ok_or_else(|| anyhow!("转换图格式记录缺失"))?;
            let target_size = target_size.ok_or_else(|| anyhow!("转换图大小记录缺失"))?;
            verify_target_blob(&storage, &target_key, &target_format, target_size).await?;
            Ok::<_, anyhow::Error>((target_key, target_format, target_size))
        }
        .await;
        match verified {
            Ok((target_key, target_format, target_size)) => {
                prepared.push((id, source_key, target_key, target_format, target_size))
            }
            Err(error) => validation_failures.push(
                serde_json::json!({"photoId": id, "error": format!("目标图验证失败：{error:#}")}),
            ),
        }
    }
    if !validation_failures.is_empty() {
        return Ok(Json(
            serde_json::json!({"removed": 0, "failures": validation_failures}),
        ));
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
    let result = drain_source_deletion_outbox(&state.storage, &state.db, Some(&job_id))
        .await
        .map_err(AppError::internal)?;
    Ok(Json(
        serde_json::json!({"removed": result.removed, "failures": result.failures}),
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

fn encode_thumbnail(input: &[u8], longest_edge: u32) -> Result<Vec<u8>> {
    if longest_edge == 0 {
        bail!("thumbnail edge must be positive");
    }
    let image = image::load_from_memory(input)?;
    let width = image.width();
    let height = image.height();
    let longest = width.max(height);
    let thumbnail = if longest > longest_edge {
        let target_width = ((width as u64 * longest_edge as u64) / longest as u64).max(1) as u32;
        let target_height = ((height as u64 * longest_edge as u64) / longest as u64).max(1) as u32;
        image.resize_exact(
            target_width,
            target_height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };
    let mut output = Cursor::new(Vec::new());
    thumbnail.write_to(&mut output, ImageFormat::WebP)?;
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
    sqlx::query("PRAGMA foreign_keys = ON; CREATE TABLE IF NOT EXISTS app_settings (key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS administrators (id INTEGER PRIMARY KEY CHECK(id=1),username TEXT NOT NULL UNIQUE,password_hash TEXT NOT NULL,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS admin_sessions (token_hash TEXT PRIMARY KEY,administrator_id INTEGER NOT NULL REFERENCES administrators(id) ON DELETE CASCADE CHECK(administrator_id=1),csrf_hash TEXT NOT NULL,created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS idx_admin_sessions_expires_at ON admin_sessions(expires_at); CREATE TABLE IF NOT EXISTS albums (id TEXT PRIMARY KEY,name TEXT NOT NULL,created_at INTEGER NOT NULL,display_created_date TEXT,photo_date_start TEXT,photo_date_end TEXT); CREATE TABLE IF NOT EXISTS photos (id TEXT PRIMARY KEY,album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,original_name TEXT NOT NULL,storage_key TEXT NOT NULL UNIQUE,format TEXT NOT NULL CHECK(format IN ('png','jpg','webp')),content_type TEXT NOT NULL,byte_size INTEGER NOT NULL,width INTEGER NOT NULL DEFAULT 0,height INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS pending_blobs (key TEXT PRIMARY KEY,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS conversion_jobs (id TEXT PRIMARY KEY,status TEXT NOT NULL,target_format TEXT NOT NULL,total INTEGER NOT NULL,completed INTEGER NOT NULL DEFAULT 0,succeeded INTEGER NOT NULL DEFAULT 0,failed INTEGER NOT NULL DEFAULT 0,cancelled INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,sources_deleted_at INTEGER); CREATE TABLE IF NOT EXISTS conversion_items (id TEXT PRIMARY KEY,job_id TEXT NOT NULL REFERENCES conversion_jobs(id) ON DELETE CASCADE,source_photo_id TEXT REFERENCES photos(id) ON DELETE SET NULL,target_photo_id TEXT REFERENCES photos(id) ON DELETE SET NULL,status TEXT NOT NULL,error TEXT); CREATE TABLE IF NOT EXISTS source_deletion_outbox (job_id TEXT NOT NULL REFERENCES conversion_jobs(id) ON DELETE CASCADE,source_photo_id TEXT NOT NULL,source_key TEXT NOT NULL,target_key TEXT NOT NULL,target_format TEXT NOT NULL,target_size INTEGER NOT NULL,created_at INTEGER NOT NULL,PRIMARY KEY(job_id,source_photo_id));").execute(pool).await?;
    let album_columns: HashSet<String> = sqlx::query("PRAGMA table_info(albums)")
        .fetch_all(pool)
        .await?
        .iter()
        .map(|row| row.get("name"))
        .collect();
    for (column, migration) in [
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
    ] {
        if !album_columns.contains(column) {
            sqlx::query(migration).execute(pool).await?;
        }
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
    sqlx::query("DELETE FROM admin_sessions WHERE expires_at<=?")
        .bind(now())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM admin_sessions WHERE administrator_id=1 AND token_hash NOT IN (SELECT token_hash FROM admin_sessions WHERE administrator_id=1 ORDER BY created_at DESC,rowid DESC LIMIT 16)")
        .execute(pool)
        .await?;
    Ok(())
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
    let master_key = load_or_create_master_key(&config.master_key_file).await?;
    let storage = StorageService::new(db.clone(), master_key);
    let photo_graph_lock = Arc::new(tokio::sync::Mutex::new(()));
    match recover_pending_blobs(&storage, &db, true).await {
        Ok(count) if count > 0 => info!(count, "recovered pending storage objects"),
        Ok(_) => {}
        Err(error) => warn!("pending storage recovery deferred: {error:#}"),
    }
    {
        let _storage_guard = storage.gate.write().await;
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
            let _photo_graph_guard = janitor_photo_graph_lock.lock().await;
            let _storage_guard = janitor_storage.gate.write().await;
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
    let state = AppState {
        db,
        storage,
        secure_cookies: config.secure_cookies,
        trust_proxy_headers: config.trust_proxy_headers,
        workers: config.workers,
        jobs: Arc::new(DashMap::new()),
        conversion_slots: Arc::new(tokio::sync::Semaphore::new(config.workers)),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(1)),
        thumbnail_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        password_hash_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        photo_graph_lock,
    };
    let api = Router::new()
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
        .route("/api/albums", get(list_albums).post(create_album))
        .route(
            "/api/albums/{album_id}",
            get(album_detail).patch(patch_album_dates),
        )
        .route(
            "/api/albums/{album_id}/photos",
            get(album_photos)
                .post(upload_photos)
                .layer(DefaultBodyLimit::max(400 * 1024 * 1024)),
        )
        .route("/api/photos", get(list_photos))
        .route("/api/photos/{photo_id}/file", get(photo_file))
        .route("/api/photos/{photo_id}/thumbnail", get(photo_thumbnail))
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

    async fn test_state() -> AppState {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        setup_database(&db).await.unwrap();
        AppState {
            storage: StorageService::new(db.clone(), [7u8; 32]),
            db,
            secure_cookies: Some(false),
            trust_proxy_headers: false,
            workers: 2,
            jobs: Arc::new(DashMap::new()),
            conversion_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            upload_slots: Arc::new(tokio::sync::Semaphore::new(1)),
            thumbnail_slots: Arc::new(tokio::sync::Semaphore::new(1)),
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

    #[test]
    fn thumbnail_is_webp_and_limits_the_longest_edge() {
        let thumbnail = encode_thumbnail(&png_fixture(1600, 800), 720).unwrap();
        assert_eq!(image::guess_format(&thumbnail).unwrap(), ImageFormat::WebP);
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
            headers.insert(
                "x-requested-with",
                HeaderValue::from_static(REQUESTED_WITH),
            );
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

    #[tokio::test]
    async fn setup_database_adds_album_date_metadata_without_losing_rows() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE albums (id TEXT PRIMARY KEY,name TEXT NOT NULL,created_at INTEGER NOT NULL); INSERT INTO albums(id,name,created_at) VALUES('legacy-album','Legacy Album',12345);")
            .execute(&pool)
            .await
            .unwrap();

        setup_database(&pool).await.unwrap();

        let legacy = sqlx::query("SELECT name,created_at,display_created_date,photo_date_start,photo_date_end FROM albums WHERE id='legacy-album'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(legacy.get::<String, _>("name"), "Legacy Album");
        assert_eq!(legacy.get::<i64, _>("created_at"), 12345);
        assert_eq!(
            legacy.get::<Option<String>, _>("display_created_date"),
            None
        );
        assert_eq!(legacy.get::<Option<String>, _>("photo_date_start"), None);
        assert_eq!(legacy.get::<Option<String>, _>("photo_date_end"), None);

        sqlx::query("UPDATE albums SET display_created_date='2020-01-02',photo_date_start='2019-01-01',photo_date_end='2020-01-01' WHERE id='legacy-album'")
            .execute(&pool)
            .await
            .unwrap();
        setup_database(&pool).await.unwrap();

        let migrated = sqlx::query("SELECT name,created_at,display_created_date,photo_date_start,photo_date_end FROM albums WHERE id='legacy-album'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(migrated.get::<String, _>("name"), "Legacy Album");
        assert_eq!(migrated.get::<i64, _>("created_at"), 12345);
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
        let date_columns = sqlx::query("PRAGMA table_info(albums)")
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .filter(|column| {
                matches!(
                    column.get::<String, _>("name").as_str(),
                    "display_created_date" | "photo_date_start" | "photo_date_end"
                )
            })
            .count();
        assert_eq!(date_columns, 3);
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
