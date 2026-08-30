//! Persistent, local-only downloadable album snapshots. Never writes to the photo store.
use super::*;
use std::io::{Read, Seek};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub struct Service {
    root: PathBuf,
    tasks: DashMap<String, CancellationToken>,
    image_slots: Arc<tokio::sync::Semaphore>,
    disk_gate: Arc<std::sync::Mutex<()>>,
}

impl Service {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            tasks: DashMap::new(),
            image_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            disk_gate: Arc::new(std::sync::Mutex::new(())),
        }
    }
    fn path(&self, id: &str, suffix: &str) -> Result<PathBuf> {
        Uuid::parse_str(id).context("invalid archive id")?;
        if !matches!(suffix, "zip" | "part" | "work") {
            bail!("invalid archive suffix");
        }
        Ok(self.root.join(format!("{id}.{suffix}")))
    }
    async fn remove_files(&self, id: &str) -> Result<()> {
        for suffix in ["zip", "part", "work"] {
            let path = self.path(id, suffix)?;
            match tokio::fs::symlink_metadata(&path).await {
                Ok(meta) => {
                    // Only UUID-named entries inside this application's dedicated directory.
                    if meta.is_dir() && !meta.file_type().is_symlink() {
                        tokio::fs::remove_dir_all(path).await?;
                    } else {
                        tokio::fs::remove_file(path).await?;
                    }
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

pub async fn setup(db: &SqlitePool) -> Result<()> {
    sqlx::raw_sql(r#"
        CREATE TABLE IF NOT EXISTS album_download_settings (
          album_id TEXT PRIMARY KEY REFERENCES albums(id) ON DELETE CASCADE,
          enabled INTEGER NOT NULL DEFAULT 0, formats TEXT NOT NULL DEFAULT '["webp"]',
          max_image_bytes INTEGER NOT NULL DEFAULT 5000000, max_zip_bytes INTEGER NOT NULL DEFAULT 0,
          revision INTEGER NOT NULL DEFAULT 1, updated_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS album_download_jobs (
          id TEXT PRIMARY KEY, album_id TEXT NOT NULL, format TEXT NOT NULL,
          revision INTEGER NOT NULL, status TEXT NOT NULL DEFAULT 'queued',
          total INTEGER NOT NULL DEFAULT 0, completed INTEGER NOT NULL DEFAULT 0,
          byte_size INTEGER NOT NULL DEFAULT 0, error TEXT, created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL, UNIQUE(album_id,format,revision));
        CREATE INDEX IF NOT EXISTS idx_album_download_jobs_status ON album_download_jobs(status);
        CREATE TRIGGER IF NOT EXISTS downloads_photo_insert AFTER INSERT ON photos BEGIN
          UPDATE album_download_settings SET revision=revision+1,updated_at=unixepoch() WHERE album_id=NEW.album_id;
        END;
        CREATE TRIGGER IF NOT EXISTS downloads_photo_delete AFTER DELETE ON photos BEGIN
          UPDATE album_download_settings SET revision=revision+1,updated_at=unixepoch() WHERE album_id=OLD.album_id;
        END;
        CREATE TRIGGER IF NOT EXISTS downloads_photo_update AFTER UPDATE OF album_id,storage_key,original_name,format ON photos BEGIN
          UPDATE album_download_settings SET revision=revision+1,updated_at=unixepoch() WHERE album_id IN (OLD.album_id,NEW.album_id);
        END;
        CREATE TRIGGER IF NOT EXISTS downloads_album_rename AFTER UPDATE OF name ON albums WHEN NEW.name<>OLD.name BEGIN
          UPDATE album_download_settings SET revision=revision+1,updated_at=unixepoch() WHERE album_id=NEW.id;
        END;
    "#).execute(db).await?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsInput {
    enabled: bool,
    formats: Vec<String>,
    max_image_bytes: i64,
    #[serde(default)]
    max_zip_bytes: i64,
}

fn validate(mut input: SettingsInput) -> ApiResult<SettingsInput> {
    if input.formats.is_empty() || input.formats.len() > 4 {
        return Err(AppError::bad("请选择至少一种格式"));
    }
    input.formats = input
        .formats
        .into_iter()
        .map(|format| format.to_lowercase())
        .collect();
    if input
        .formats
        .iter()
        .any(|format| !matches!(format.as_str(), "png" | "jpg" | "jpeg" | "webp"))
    {
        return Err(AppError::bad("仅支持 PNG、JPG、JPEG 和 WebP"));
    }
    input.formats.sort();
    input.formats.dedup();
    if input.max_image_bytes != 0 && !(16_384..=500_000_000).contains(&input.max_image_bytes) {
        return Err(AppError::bad(
            "单张图片上限须为 16 KB 至 500 MB，0 表示不限",
        ));
    }
    if input.max_zip_bytes != 0 && !(1_000_000..=1_000_000_000_000).contains(&input.max_zip_bytes) {
        return Err(AppError::bad("ZIP 上限须为 1 MB 至 1 TB，0 表示不限"));
    }
    Ok(input)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/album-downloads/public", get(public_list))
        .route("/api/album-downloads", get(admin_list))
        .route(
            "/api/albums/{album_id}/download-settings",
            axum::routing::put(save_settings),
        )
        .route("/api/albums/{album_id}/downloads/rebuild", post(rebuild))
        .route("/api/albums/{album_id}/downloads/{format}", get(download))
        .route("/api/album-downloads/{job_id}/cancel", post(cancel))
        .route("/api/album-downloads/{job_id}", delete(remove))
}

async fn public_list(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let settings = sqlx::query("SELECT * FROM album_download_settings WHERE enabled=1")
        .fetch_all(&state.db)
        .await
        .map_err(AppError::internal)?;
    let jobs = sqlx::query("SELECT j.* FROM album_download_jobs j JOIN album_download_settings s ON s.album_id=j.album_id AND s.revision=j.revision WHERE s.enabled=1").fetch_all(&state.db).await.map_err(AppError::internal)?;
    let result = settings.iter().map(|s| {
        let album_id: String = s.get("album_id");
        let formats: Vec<String> = serde_json::from_str(&s.get::<String,_>("formats")).unwrap_or_default();
        let entries = formats.iter().map(|format| {
            let job = jobs.iter().find(|j| j.get::<String,_>("album_id")==album_id && j.get::<String,_>("format")==*format);
            let status: String = job.map(|j| j.get("status")).unwrap_or("queued".into());
            let size: i64 = job.map(|j| j.get("byte_size")).unwrap_or(0);
            serde_json::json!({"format":format,"status":status,"byteSize":size,"url": if status=="ready" {job.map(|j| format!("/api/albums/{album_id}/downloads/{format}?version={}", j.get::<String,_>("id")))} else {None}})
        }).collect::<Vec<_>>();
        serde_json::json!({"albumId":album_id,"formats":entries})
    }).collect::<Vec<_>>();
    Ok(Json(serde_json::json!(result)))
}

async fn admin_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, false).await?;
    let settings = sqlx::query("SELECT a.id,a.name,s.* FROM albums a LEFT JOIN album_download_settings s ON s.album_id=a.id ORDER BY a.position,a.created_at DESC").fetch_all(&state.db).await.map_err(AppError::internal)?;
    let jobs = sqlx::query("SELECT j.*,a.name album_name FROM album_download_jobs j JOIN albums a ON a.id=j.album_id ORDER BY j.created_at DESC LIMIT 500").fetch_all(&state.db).await.map_err(AppError::internal)?;
    let settings = settings.iter().map(|r| serde_json::json!({
        "albumId":r.get::<String,_>("id"),"albumName":r.get::<String,_>("name"),
        "enabled":r.get::<Option<bool>,_>("enabled").unwrap_or(false),
        "formats":serde_json::from_str::<Vec<String>>(&r.get::<Option<String>,_>("formats").unwrap_or("[\"webp\"]".into())).unwrap_or_default(),
        "maxImageBytes":r.get::<Option<i64>,_>("max_image_bytes").unwrap_or(5_000_000),
        "maxZipBytes":r.get::<Option<i64>,_>("max_zip_bytes").unwrap_or(0),
        "revision":r.get::<Option<i64>,_>("revision").unwrap_or(0)
    })).collect::<Vec<_>>();
    let jobs = jobs.iter().map(|r| serde_json::json!({
        "id":r.get::<String,_>("id"),"albumId":r.get::<String,_>("album_id"),"albumName":r.get::<String,_>("album_name"),
        "format":r.get::<String,_>("format"),"revision":r.get::<i64,_>("revision"),"status":r.get::<String,_>("status"),
        "total":r.get::<i64,_>("total"),"completed":r.get::<i64,_>("completed"),"byteSize":r.get::<i64,_>("byte_size"),
        "error":r.get::<Option<String>,_>("error"),"createdAt":r.get::<i64,_>("created_at"),"updatedAt":r.get::<i64,_>("updated_at")
    })).collect::<Vec<_>>();
    let bytes: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(byte_size),0) FROM album_download_jobs WHERE status='ready'",
    )
    .fetch_one(&state.db)
    .await
    .map_err(AppError::internal)?;
    Ok(Json(
        serde_json::json!({"settings":settings,"jobs":jobs,"localBytes":bytes,"directory":"data/album-downloads"}),
    ))
}

async fn save_settings(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
    headers: HeaderMap,
    Json(input): Json<SettingsInput>,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, true).await?;
    let input = validate(input)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM albums WHERE id=?)")
        .bind(&album_id)
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    if !exists {
        return Err(AppError::bad("相簿不存在"));
    }
    sqlx::query("INSERT INTO album_download_settings(album_id,enabled,formats,max_image_bytes,max_zip_bytes,updated_at) VALUES(?,?,?,?,?,?) ON CONFLICT(album_id) DO UPDATE SET enabled=excluded.enabled,formats=excluded.formats,max_image_bytes=excluded.max_image_bytes,max_zip_bytes=excluded.max_zip_bytes,revision=revision+1,updated_at=excluded.updated_at")
        .bind(&album_id).bind(input.enabled).bind(serde_json::to_string(&input.formats).map_err(AppError::internal)?).bind(input.max_image_bytes).bind(input.max_zip_bytes).bind(now()-4).execute(&state.db).await.map_err(AppError::internal)?;
    Ok(Json(serde_json::json!({"queued":input.enabled})))
}

async fn rebuild(
    State(state): State<AppState>,
    AxumPath(album_id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, true).await?;
    let count = sqlx::query("UPDATE album_download_settings SET revision=revision+1,updated_at=? WHERE album_id=? AND enabled=1").bind(now()-4).bind(album_id).execute(&state.db).await.map_err(AppError::internal)?.rows_affected();
    if count == 0 {
        return Err(AppError::bad("请先启用相簿下载"));
    }
    Ok(Json(serde_json::json!({"queued":true})))
}

async fn cancel(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, true).await?;
    sqlx::query("UPDATE album_download_jobs SET status='cancelled',updated_at=? WHERE id=? AND status IN ('queued','running')").bind(now()).bind(&id).execute(&state.db).await.map_err(AppError::internal)?;
    if let Some(token) = state.downloads.tasks.get(&id) {
        token.cancel();
    }
    Ok(Json(serde_json::json!({"cancelled":true})))
}

async fn remove(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&headers, &state, true).await?;
    state
        .downloads
        .path(&id, "zip")
        .map_err(|_| AppError::bad("无效的压缩包编号"))?;
    sqlx::query("UPDATE album_download_jobs SET status='deleting',updated_at=? WHERE id=?")
        .bind(now())
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(AppError::internal)?;
    if let Some(token) = state.downloads.tasks.get(&id) {
        token.cancel();
    }
    Ok(Json(serde_json::json!({"deleting":true})))
}

#[derive(Deserialize)]
struct DownloadQuery {
    version: Option<String>,
}

async fn download(
    State(state): State<AppState>,
    AxumPath((album_id, format)): AxumPath<(String, String)>,
    Query(query): Query<DownloadQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let row = sqlx::query("SELECT j.id,a.name FROM album_download_jobs j JOIN album_download_settings s ON s.album_id=j.album_id AND s.revision=j.revision JOIN albums a ON a.id=j.album_id WHERE j.album_id=? AND j.format=? AND j.status='ready' AND s.enabled=1")
        .bind(album_id).bind(&format).fetch_optional(&state.db).await.map_err(AppError::internal)?
        .ok_or_else(|| AppError {status:StatusCode::NOT_FOUND,message:"此相簿压缩包尚不可下载或已被管理员撤下".into(),clear_auth_cookies:None})?;
    let job_id: String = row.get("id");
    if query
        .version
        .as_ref()
        .is_some_and(|version| version != &job_id)
    {
        return Err(AppError {
            status: StatusCode::NOT_FOUND,
            message: "压缩包版本已更新，请重新选择下载".into(),
            clear_auth_cookies: None,
        });
    }
    let path = state
        .downloads
        .path(&row.get::<String, _>("id"), "zip")
        .map_err(AppError::internal)?;
    let mut file = tokio::fs::File::open(path).await.map_err(|_| AppError {
        status: StatusCode::NOT_FOUND,
        message: "压缩包已删除，请管理员重新生成".into(),
        clear_auth_cookies: None,
    })?;
    let size = file.metadata().await.map_err(AppError::internal)?.len();
    let etag = format!("\"{job_id}-{size}\"");
    let range = if headers
        .get(header::IF_RANGE)
        .is_some_and(|value| value.to_str().ok() != Some(etag.as_str()))
    {
        None
    } else {
        headers.get(header::RANGE).and_then(|h| h.to_str().ok())
    };
    let (start, end, partial) = match parse_range(range, size) {
        Ok(value) => value,
        Err(error) => {
            let mut response = error.into_response();
            response.headers_mut().insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{size}")).map_err(AppError::internal)?,
            );
            return Ok(response);
        }
    };
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(AppError::internal)?;
    let length = end - start + 1;
    let mut response = Body::from_stream(ReaderStream::new(file.take(length))).into_response();
    *response.status_mut() = if partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let filename = format!(
        "{}-{format}.zip",
        sanitize_export_name(&row.get::<String, _>("name"), "album")
    );
    let out = response.headers_mut();
    out.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).map_err(AppError::internal)?,
    );
    out.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    out.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    out.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    out.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).map_err(AppError::internal)?,
    );
    out.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"album-{format}.zip\"; filename*=UTF-8''{}",
            urlencoding::encode(&filename)
        ))
        .map_err(AppError::internal)?,
    );
    if partial {
        out.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{size}"))
                .map_err(AppError::internal)?,
        );
    }
    Ok(response)
}

fn parse_range(range: Option<&str>, size: u64) -> ApiResult<(u64, u64, bool)> {
    let invalid = || AppError {
        status: StatusCode::RANGE_NOT_SATISFIABLE,
        message: "无效下载范围".into(),
        clear_auth_cookies: None,
    };
    if size == 0 {
        return Err(invalid());
    }
    let Some(range) = range else {
        return Ok((0, size - 1, false));
    };
    let (a, b) = range
        .strip_prefix("bytes=")
        .and_then(|v| v.split_once('-'))
        .ok_or_else(invalid)?;
    let (start, end) = if a.is_empty() {
        let tail = b.parse::<u64>().map_err(|_| invalid())?;
        if tail == 0 {
            return Err(invalid());
        }
        (size.saturating_sub(tail), size - 1)
    } else {
        (
            a.parse::<u64>().map_err(|_| invalid())?,
            if b.is_empty() {
                size - 1
            } else {
                b.parse::<u64>().map_err(|_| invalid())?.min(size - 1)
            },
        )
    };
    if start >= size || start > end {
        return Err(invalid());
    }
    Ok((start, end, true))
}

pub async fn start(state: AppState) -> Result<()> {
    tokio::fs::create_dir_all(&state.downloads.root).await?;
    sqlx::query("UPDATE album_download_jobs SET status='queued',completed=0,error=NULL WHERE status='running'").execute(&state.db).await?;
    // Recover only our own UUID files; never enumerate or delete S3/WebDAV objects.
    let mut entries = tokio::fs::read_dir(&state.downloads.root).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((id, suffix)) = name.rsplit_once('.') else {
            continue;
        };
        if Uuid::parse_str(id).is_err() || !matches!(suffix, "zip" | "part" | "work") {
            continue;
        }
        let ready: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM album_download_jobs WHERE id=? AND status='ready')",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await?;
        if !ready {
            state.downloads.remove_files(id).await?;
        } else if suffix != "zip" {
            let path = state.downloads.path(id, suffix)?;
            let meta = tokio::fs::symlink_metadata(&path).await?;
            if meta.is_dir() && !meta.file_type().is_symlink() {
                tokio::fs::remove_dir_all(path).await?;
            } else {
                tokio::fs::remove_file(path).await?;
            }
        }
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            if let Err(error) = tick(&state).await {
                warn!("album download scheduler: {error:#}");
            }
        }
    });
    Ok(())
}

async fn tick(state: &AppState) -> Result<()> {
    sqlx::query("UPDATE album_download_jobs SET status='deleting',updated_at=? WHERE status NOT IN ('deleted','deleting') AND NOT EXISTS(SELECT 1 FROM album_download_settings s WHERE s.album_id=album_download_jobs.album_id AND s.revision=album_download_jobs.revision AND s.enabled=1)").bind(now()).execute(&state.db).await?;
    let deleting = sqlx::query_scalar::<_, String>(
        "SELECT id FROM album_download_jobs WHERE status IN ('deleting','cancelled','failed')",
    )
    .fetch_all(&state.db)
    .await?;
    for id in deleting {
        if let Some(token) = state.downloads.tasks.get(&id) {
            token.cancel();
            continue;
        }
        state.downloads.remove_files(&id).await?;
        sqlx::query("UPDATE album_download_jobs SET status=CASE WHEN status='deleting' THEN 'deleted' ELSE status END,byte_size=0 WHERE id=?").bind(id).execute(&state.db).await?;
    }
    let configs =
        sqlx::query("SELECT * FROM album_download_settings WHERE enabled=1 AND updated_at<=?")
            .bind(now() - 3)
            .fetch_all(&state.db)
            .await?;
    for config in configs {
        let formats: Vec<String> = serde_json::from_str(&config.get::<String, _>("formats"))?;
        for format in formats {
            sqlx::query("INSERT OR IGNORE INTO album_download_jobs(id,album_id,format,revision,created_at,updated_at) VALUES(?,?,?,?,?,?)")
                .bind(Uuid::new_v4().to_string()).bind(config.get::<String,_>("album_id")).bind(format).bind(config.get::<i64,_>("revision")).bind(now()).bind(now()).execute(&state.db).await?;
        }
    }
    let jobs = sqlx::query_scalar::<_, String>(
        "SELECT id FROM album_download_jobs WHERE status='queued' ORDER BY created_at,id LIMIT 8",
    )
    .fetch_all(&state.db)
    .await?;
    for id in jobs {
        if state.downloads.tasks.len() >= 2 {
            break;
        }
        if state.downloads.tasks.contains_key(&id) {
            continue;
        }
        let token = CancellationToken::new();
        state.downloads.tasks.insert(id.clone(), token.clone());
        let state = state.clone();
        tokio::spawn(async move {
            let inner = state.clone();
            let job_id = id.clone();
            let task_token = token.clone();
            let result =
                tokio::spawn(async move { build(&inner, &job_id, task_token).await }).await;
            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(format!("{error:#}")),
                Err(error) => Some(error.to_string()),
            };
            if let Some(error) = error {
                let status = if token.is_cancelled() {
                    "cancelled"
                } else {
                    "failed"
                };
                let _=sqlx::query("UPDATE album_download_jobs SET status=?,error=?,updated_at=? WHERE id=? AND status IN ('running','queued')").bind(status).bind(error.chars().take(1000).collect::<String>()).bind(now()).bind(&id).execute(&state.db).await;
            }
            state.downloads.tasks.remove(&id);
        });
    }
    Ok(())
}

fn check_cancel(token: &CancellationToken) -> Result<()> {
    if token.is_cancelled() {
        bail!("打包已取消");
    }
    Ok(())
}

fn encode_download(
    input: &[u8],
    format: &str,
    max_bytes: i64,
    token: &CancellationToken,
) -> Result<Vec<u8>> {
    check_cancel(token)?;
    let target = match format {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::WebP,
        _ => bail!("不支持的打包格式"),
    };
    // Avoid a second lossy encoding when the original already meets both requirements.
    if image::guess_format(input)? == target && (max_bytes == 0 || input.len() as i64 <= max_bytes)
    {
        return Ok(input.to_vec());
    }
    let mut image = image::load_from_memory(input)?;
    for _ in 0..24 {
        check_cancel(token)?;
        let qualities = if format == "png" {
            vec![90]
        } else {
            vec![92, 82, 70, 55, 40]
        };
        for quality in qualities {
            check_cancel(token)?;
            let mut data = Vec::new();
            match format {
                "webp" => {
                    let rgba = image.to_rgba8();
                    data = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
                        .encode(quality as f32)
                        .to_vec();
                }
                "png" => image.write_to(&mut Cursor::new(&mut data), ImageFormat::Png)?,
                "jpg" | "jpeg" => {
                    let rgba = image.to_rgba8();
                    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
                    for (out, pixel) in rgb.pixels_mut().zip(rgba.pixels()) {
                        let a = pixel[3] as u32;
                        for channel in 0..3 {
                            out[channel] =
                                ((pixel[channel] as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
                        }
                    }
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, quality)
                        .encode_image(&rgb)?;
                }
                _ => bail!("不支持的打包格式"),
            }
            if max_bytes == 0 || data.len() as i64 <= max_bytes {
                return Ok(data);
            }
        }
        if image.width() == 1 && image.height() == 1 {
            break;
        }
        image = image.resize_exact(
            (image.width() * 3 / 4).max(1),
            (image.height() * 3 / 4).max(1),
            image::imageops::FilterType::Lanczos3,
        );
    }
    bail!("无法达到单张图片大小限制，请提高上限")
}

fn check_disk(path: &Path, needed: u64) -> Result<()> {
    const RESERVE: u64 = 256 * 1024 * 1024;
    if fs2::available_space(path)? < needed.saturating_add(RESERVE) {
        bail!("本地磁盘空间不足，已停止打包并保留 256 MiB 安全空间；请清理旧 ZIP 后重新生成");
    }
    Ok(())
}

fn write_zip(
    path: &Path,
    photos: &[PreparedArchivePhoto],
    limit: i64,
    token: &CancellationToken,
    disk_gate: &std::sync::Mutex<()>,
) -> Result<u64> {
    let mut archive = ZipWriter::new(std::fs::File::create(path)?);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true)
        .unix_permissions(0o644);
    let mut buffer = vec![0; 64 * 1024];
    let mut payload_bytes = 0u64;
    for photo in photos {
        check_cancel(token)?;
        {
            let _guard = disk_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("disk lock poisoned"))?;
            check_disk(path.parent().context("missing ZIP directory")?, 4096)?;
            archive.start_file(&photo.archive_name, options)?;
        }
        let mut source = std::fs::File::open(&photo.source_path)?;
        loop {
            check_cancel(token)?;
            let n = source.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            {
                let _guard = disk_gate
                    .lock()
                    .map_err(|_| anyhow::anyhow!("disk lock poisoned"))?;
                check_disk(path.parent().context("missing ZIP directory")?, n as u64)?;
                archive.write_all(&buffer[..n])?;
            }
            payload_bytes += n as u64;
            if limit > 0 && payload_bytes > limit as u64 {
                bail!("整个 ZIP 超过大小上限；未发布，请提高上限或降低单张图片大小");
            }
        }
    }
    let _guard = disk_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("disk lock poisoned"))?;
    check_disk(
        path.parent().context("missing ZIP directory")?,
        photos
            .iter()
            .map(|p| p.archive_name.len() as u64 + 128)
            .sum(),
    )?;
    let mut file = archive.finish()?;
    file.flush()?;
    file.sync_all()?;
    let size = file.stream_position()?;
    if limit > 0 && size > limit as u64 {
        bail!("ZIP 连同目录信息超过大小上限，未发布");
    }
    Ok(size)
}

async fn build(state: &AppState, id: &str, token: CancellationToken) -> Result<()> {
    let token = token.child_token();
    let config=sqlx::query("SELECT j.album_id,j.format,j.revision,s.max_image_bytes,s.max_zip_bytes FROM album_download_jobs j JOIN album_download_settings s ON s.album_id=j.album_id AND s.revision=j.revision WHERE j.id=? AND j.status='queued' AND s.enabled=1").bind(id).fetch_optional(&state.db).await?.context("任务已失效")?;
    let album_id: String = config.get("album_id");
    let format: String = config.get("format");
    let revision: i64 = config.get("revision");
    let max_image: i64 = config.get("max_image_bytes");
    let max_zip: i64 = config.get("max_zip_bytes");
    let photos=sqlx::query("SELECT id,album_id,original_name,storage_key,format,content_type,byte_size,width,height,created_at FROM photos WHERE album_id=? ORDER BY created_at,id").bind(&album_id).fetch_all(&state.db).await?.iter().map(photo_from).collect::<Vec<_>>();
    if photos.is_empty() {
        bail!("相簿没有图片，上传后将自动生成");
    }
    let affected=sqlx::query("UPDATE album_download_jobs SET status='running',total=?,completed=0,error=NULL,updated_at=? WHERE id=? AND status='queued'").bind(photos.len() as i64).bind(now()).bind(id).execute(&state.db).await?.rows_affected();
    if affected == 0 {
        bail!("任务已取消");
    }
    state.downloads.remove_files(id).await?;
    let work = state.downloads.path(id, "work")?;
    tokio::fs::create_dir_all(&work).await?;
    let mut names = HashSet::new();
    let inputs = photos
        .into_iter()
        .enumerate()
        .map(|(index, photo)| {
            let name = unique_export_name(
                sanitize_export_name(&renamed(&photo.original_name, &format), "image"),
                &mut names,
            );
            (index, photo, name)
        })
        .collect::<Vec<_>>();
    let mut queue=stream::iter(inputs.into_iter().map(|(index,photo,name)|{
        let token=token.clone();let path=work.join(format!("{index}.{format}"));let format=format.clone();
        async move {
            check_cancel(&token)?;
            let _slot=state.downloads.image_slots.clone().acquire_owned().await?;
            check_cancel(&token)?;
            let data={let _guard=state.storage.gate.read().await;state.storage.store().await?.get(&photo.storage_key).await?};
            let path_copy=path.clone();let disk_gate=state.downloads.disk_gate.clone();
            tokio::task::spawn_blocking(move ||->Result<()> {let data=encode_download(&data,&format,max_image,&token)?;check_cancel(&token)?;let _guard=disk_gate.lock().map_err(|_|anyhow::anyhow!("disk lock poisoned"))?;check_disk(path_copy.parent().context("missing work directory")?,data.len() as u64)?;std::fs::write(path_copy,data)?;Ok(())}).await??;
            sqlx::query("UPDATE album_download_jobs SET completed=completed+1,updated_at=? WHERE id=? AND status='running'").bind(now()).bind(id).execute(&state.db).await?;
            Ok::<_,anyhow::Error>((index,PreparedArchivePhoto{source_path:path,archive_name:name}))
        }
    })).buffer_unordered(4);
    let mut prepared = Vec::new();
    let mut first_error = None;
    while let Some(result) = queue.next().await {
        match result {
            Ok(photo) => prepared.push(photo),
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
                token.cancel();
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    check_cancel(&token)?;
    prepared.sort_by_key(|p| p.0);
    let prepared = prepared.into_iter().map(|p| p.1).collect::<Vec<_>>();
    let part = state.downloads.path(id, "part")?;
    let zip = state.downloads.path(id, "zip")?;
    let part_copy = part.clone();
    let zip_token = token.clone();
    let disk_gate = state.downloads.disk_gate.clone();
    let size = tokio::task::spawn_blocking(move || {
        write_zip(&part_copy, &prepared, max_zip, &zip_token, &disk_gate)
    })
    .await??;
    check_cancel(&token)?;
    tokio::fs::rename(&part, &zip).await?;
    // A snapshot may become obsolete during encoding. Never publish it afterwards.
    let published=sqlx::query("UPDATE album_download_jobs SET status='ready',byte_size=?,updated_at=? WHERE id=? AND status='running' AND EXISTS(SELECT 1 FROM album_download_settings s WHERE s.album_id=? AND s.revision=? AND s.enabled=1)").bind(size as i64).bind(now()).bind(id).bind(&album_id).bind(revision).execute(&state.db).await?.rows_affected();
    if published == 0 {
        state.downloads.remove_files(id).await?;
        bail!("相簿已修改，正在准备新版压缩包");
    }
    tokio::fs::remove_dir_all(&work).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limits_formats_and_path_validation() {
        let input = SettingsInput {
            enabled: true,
            formats: vec!["PNG".into(), "jpeg".into()],
            max_image_bytes: 20_000,
            max_zip_bytes: 0,
        };
        assert_eq!(
            validate(input.clone()).unwrap().formats,
            vec!["jpeg", "png"]
        );
        assert!(
            validate(SettingsInput {
                max_image_bytes: 1,
                ..input.clone()
            })
            .is_err()
        );
        assert!(
            validate(SettingsInput {
                formats: vec!["gif".into()],
                ..input
            })
            .is_err()
        );
        assert!(
            Service::new(PathBuf::from("/safe/album-downloads"))
                .path("../other", "zip")
                .is_err()
        );
    }
    #[test]
    fn download_ranges_are_bounded() {
        assert_eq!(parse_range(Some("bytes=2-8"), 10).unwrap(), (2, 8, true));
        assert_eq!(parse_range(Some("bytes=-3"), 10).unwrap(), (7, 9, true));
        assert!(parse_range(Some("bytes=10-"), 10).is_err());
        assert!(parse_range(Some("bytes=0-1,3-4"), 10).is_err());
    }
    #[test]
    fn every_export_format_obeys_the_image_limit() {
        let mut image = image::RgbaImage::new(256, 256);
        for (x, y, p) in image.enumerate_pixels_mut() {
            *p = image::Rgba([
                (x * 13 + y * 7) as u8,
                (x * 17 + y * 11) as u8,
                (x ^ y) as u8,
                170,
            ]);
        }
        let mut source = Cursor::new(Vec::new());
        image.write_to(&mut source, ImageFormat::Png).unwrap();
        assert_eq!(
            encode_download(source.get_ref(), "png", 0, &CancellationToken::new()).unwrap(),
            *source.get_ref()
        );
        for format in ["png", "jpg", "jpeg", "webp"] {
            let data = encode_download(source.get_ref(), format, 20_000, &CancellationToken::new())
                .unwrap();
            assert!(data.len() <= 20_000);
            assert_eq!(
                encode_download(&data, format, 20_000, &CancellationToken::new()).unwrap(),
                data
            );
            assert_eq!(
                image::guess_format(&data).unwrap(),
                match format {
                    "png" => ImageFormat::Png,
                    "webp" => ImageFormat::WebP,
                    _ => ImageFormat::Jpeg,
                }
            );
        }
        let token = CancellationToken::new();
        token.cancel();
        assert!(encode_download(source.get_ref(), "png", 20_000, &token).is_err());
    }
    #[tokio::test]
    async fn content_changes_invalidate_published_downloads() {
        let state = super::super::tests::test_state().await;
        sqlx::query("INSERT INTO albums(id,name,created_at) VALUES('album','Album',1)")
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO album_download_settings(album_id,enabled,updated_at) VALUES('album',1,0)",
        )
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO photos(id,album_id,original_name,storage_key,format,content_type,byte_size,created_at) VALUES('photo','album','a.png','a.png','png','image/png',1,1)").execute(&state.db).await.unwrap();
        let rev: i64 = sqlx::query_scalar("SELECT revision FROM album_download_settings")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(rev, 2);
        sqlx::query("DELETE FROM photos")
            .execute(&state.db)
            .await
            .unwrap();
        let rev: i64 = sqlx::query_scalar("SELECT revision FROM album_download_settings")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(rev, 3);
    }
}
