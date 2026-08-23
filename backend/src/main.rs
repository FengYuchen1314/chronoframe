use std::{
    collections::{HashMap, HashSet},
    env,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, AeadCore, OsRng},
};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_sdk_s3::{Client as S3Client, config::Builder as S3ConfigBuilder};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use dashmap::DashMap;
use futures_util::stream::{self, StreamExt};
use image::ImageFormat;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, sqlite::SqlitePoolOptions};
use tokio::{io::AsyncWriteExt, time::timeout};
use tokio_util::sync::CancellationToken;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    storage: StorageService,
    admin_token: Arc<String>,
    workers: usize,
    jobs: Arc<DashMap<String, CancellationToken>>,
    conversion_slots: Arc<tokio::sync::Semaphore>,
    upload_slots: Arc<tokio::sync::Semaphore>,
    thumbnail_slots: Arc<tokio::sync::Semaphore>,
    photo_graph_lock: Arc<tokio::sync::Mutex<()>>,
}

const STORAGE_IO_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_UPLOAD_BATCH_BYTES: usize = 384 * 1024 * 1024;
const MAX_UPLOAD_FILES: usize = 128;

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
    admin_token: String,
    workers: usize,
    web_dir: String,
}

impl Config {
    fn from_env() -> Result<Self> {
        let get =
            |name: &str, default: &str| env::var(name).unwrap_or_else(|_| default.to_string());
        Ok(Self {
            database_url: get("CF_DATABASE_URL", "sqlite://data/chronoframe.db?mode=rwc"),
            admin_token: get("CF_ADMIN_TOKEN", "change-me"),
            workers: get("CF_CONVERSION_WORKERS", "4")
                .parse::<usize>()
                .unwrap_or(4)
                .clamp(1, 16),
            web_dir: get("CF_WEB_DIR", "./.output/public"),
        })
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
    fn new(db: SqlitePool, admin_token: &str) -> Self {
        let digest = Sha256::digest(admin_token.as_bytes());
        let mut encryption_key = [0; 32];
        encryption_key.copy_from_slice(&digest);
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
                    .context("无法解密 WebDAV 密码；管理员令牌变更后请在后台重新保存密码")?
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
                    .context("无法解密 S3 密钥；管理员令牌变更后请在后台重新保存密钥")?
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
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
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
}
impl AppError {
    fn bad(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
    fn internal(e: impl Into<anyhow::Error>) -> Self {
        let e = e.into();
        error!("{e:#}");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "服务器处理请求时出错".into(),
        }
    }
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({"error": self.message})),
        )
            .into_response()
    }
}
type ApiResult<T> = std::result::Result<T, AppError>;

async fn api_not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "API 路径不存在"})),
    )
}

fn admin(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    if headers.get("x-admin-token").and_then(|v| v.to_str().ok())
        == Some(state.admin_token.as_str())
    {
        Ok(())
    } else {
        Err(AppError {
            status: StatusCode::UNAUTHORIZED,
            message: "需要管理员令牌".into(),
        })
    }
}

async fn get_storage_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<StorageSettingsOutput>> {
    admin(&headers, &state)?;
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
    admin(&headers, &state)?;
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
    admin(&headers, &state)?;
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
async fn list_albums(State(state): State<AppState>) -> ApiResult<Json<Vec<Album>>> {
    let rows = sqlx::query("SELECT a.id,a.name,a.created_at,COUNT(p.id) photo_count FROM albums a LEFT JOIN photos p ON p.album_id=a.id GROUP BY a.id ORDER BY a.created_at DESC").fetch_all(&state.db).await.map_err(AppError::internal)?;
    Ok(Json(rows.iter().map(album_from).collect()))
}
async fn album_detail(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
) -> ApiResult<Json<AlbumDetail>> {
    let row = sqlx::query("SELECT a.id,a.name,a.created_at,COUNT(p.id) photo_count FROM albums a LEFT JOIN photos p ON p.album_id=a.id WHERE a.id=? GROUP BY a.id")
        .bind(&album_id)
        .fetch_optional(&state.db)
        .await
        .map_err(AppError::internal)?
        .ok_or_else(|| AppError {
            status: StatusCode::NOT_FOUND,
            message: "相簿不存在".into(),
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
    admin(&headers, &state)?;
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::bad("相簿名不能为空且不得超过 100 个字符"));
    }
    let album = Album {
        id: Uuid::new_v4().to_string(),
        name: name.into(),
        created_at: now(),
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
    admin(&headers, &state)?;
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
    let row = sqlx::query("SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos WHERE id=?").bind(photo_id).fetch_optional(&state.db).await.map_err(AppError::internal)?.ok_or_else(|| AppError { status: StatusCode::NOT_FOUND, message: "图片不存在".into() })?;
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
    admin(&headers, &state)?;
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
    admin(&headers, &state)?;
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
    admin(&headers, &state)?;
    let row = sqlx::query("SELECT id,status,target_format,total,completed,succeeded,failed,cancelled,created_at,updated_at,sources_deleted_at FROM conversion_jobs WHERE id=?").bind(&job_id).fetch_optional(&state.db).await.map_err(AppError::internal)?.ok_or_else(|| AppError { status: StatusCode::NOT_FOUND, message: "转换任务不存在".into() })?;
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
    admin(&headers, &state)?;
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
    admin(&headers, &state)?;
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
    sqlx::query("PRAGMA foreign_keys = ON; CREATE TABLE IF NOT EXISTS app_settings (key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS albums (id TEXT PRIMARY KEY,name TEXT NOT NULL,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS photos (id TEXT PRIMARY KEY,album_id TEXT NOT NULL REFERENCES albums(id) ON DELETE CASCADE,original_name TEXT NOT NULL,storage_key TEXT NOT NULL UNIQUE,format TEXT NOT NULL CHECK(format IN ('png','jpg','webp')),content_type TEXT NOT NULL,byte_size INTEGER NOT NULL,width INTEGER NOT NULL DEFAULT 0,height INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS pending_blobs (key TEXT PRIMARY KEY,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS conversion_jobs (id TEXT PRIMARY KEY,status TEXT NOT NULL,target_format TEXT NOT NULL,total INTEGER NOT NULL,completed INTEGER NOT NULL DEFAULT 0,succeeded INTEGER NOT NULL DEFAULT 0,failed INTEGER NOT NULL DEFAULT 0,cancelled INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,sources_deleted_at INTEGER); CREATE TABLE IF NOT EXISTS conversion_items (id TEXT PRIMARY KEY,job_id TEXT NOT NULL REFERENCES conversion_jobs(id) ON DELETE CASCADE,source_photo_id TEXT REFERENCES photos(id) ON DELETE SET NULL,target_photo_id TEXT REFERENCES photos(id) ON DELETE SET NULL,status TEXT NOT NULL,error TEXT); CREATE TABLE IF NOT EXISTS source_deletion_outbox (job_id TEXT NOT NULL REFERENCES conversion_jobs(id) ON DELETE CASCADE,source_photo_id TEXT NOT NULL,source_key TEXT NOT NULL,target_key TEXT NOT NULL,target_format TEXT NOT NULL,target_size INTEGER NOT NULL,created_at INTEGER NOT NULL,PRIMARY KEY(job_id,source_photo_id));").execute(pool).await?;
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
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=info".into()))
        .init();
    let config = Config::from_env()?;
    if config.admin_token == "change-me" || config.admin_token.contains("change-this") {
        bail!("CF_ADMIN_TOKEN must be set to a strong non-default value before startup");
    }
    if let Some(parent) = Path::new(
        &config
            .database_url
            .trim_start_matches("sqlite://")
            .split('?')
            .next()
            .unwrap_or("."),
    )
    .parent()
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    let db = SqlitePoolOptions::new()
        .max_connections(8)
        .connect(&config.database_url)
        .await?;
    setup_database(&db).await?;
    let storage = StorageService::new(db.clone(), &config.admin_token);
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
        admin_token: Arc::new(config.admin_token),
        workers: config.workers,
        jobs: Arc::new(DashMap::new()),
        conversion_slots: Arc::new(tokio::sync::Semaphore::new(config.workers)),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(1)),
        thumbnail_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        photo_graph_lock,
    };
    let api = Router::new()
        .route(
            "/api/settings/storage",
            get(get_storage_settings).put(save_storage_settings),
        )
        .route("/api/settings/storage/test", post(test_storage_settings))
        .route("/api/albums", get(list_albums).post(create_album))
        .route("/api/albums/{album_id}", get(album_detail))
        .route(
            "/api/albums/{album_id}/photos",
            get(album_photos).post(upload_photos),
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
        .route("/api/{*path}", any(api_not_found))
        .layer(DefaultBodyLimit::max(400 * 1024 * 1024));
    let web_dir = PathBuf::from(&config.web_dir);
    let index = web_dir.join("index.html");
    let app = Router::new()
        .merge(api)
        .nest_service("/_nuxt", ServeDir::new(web_dir.join("_nuxt")))
        .fallback_service(ServeDir::new(&config.web_dir).fallback(ServeFile::new(index)))
        .layer(CorsLayer::very_permissive())
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
