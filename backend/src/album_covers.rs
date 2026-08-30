use super::*;
use image::ImageDecoder;

// Cover-only uploads are small, transactional site assets. Keeping the encoded bytes in
// SQLite avoids remote-storage orphans and makes replacement/deletion crash-safe.
const COVER_EDGE: u32 = 800;
const COVER_MAX_BYTES: usize = 200_000;

pub(super) const ALBUM_SELECT: &str = "SELECT a.*,c.photo_id cover_photo_id,c.version cover_version,(SELECT id FROM photos WHERE album_id=a.id ORDER BY created_at DESC,id DESC LIMIT 1) first_photo_id,COUNT(p.id) photo_count FROM albums a LEFT JOIN photos p ON p.album_id=a.id LEFT JOIN album_covers c ON c.album_id=a.id";

pub(super) async fn setup(db: &SqlitePool) -> Result<()> {
    sqlx::raw_sql("CREATE INDEX IF NOT EXISTS idx_photos_album_cover_order ON photos(album_id,created_at DESC,id DESC);
    CREATE TABLE IF NOT EXISTS album_covers (
        album_id TEXT PRIMARY KEY REFERENCES albums(id) ON DELETE CASCADE,
        photo_id TEXT REFERENCES photos(id) ON DELETE CASCADE,
        image BLOB,
        version TEXT NOT NULL,
        CHECK ((photo_id IS NOT NULL AND image IS NULL) OR
               (photo_id IS NULL AND image IS NOT NULL AND length(image) BETWEEN 1 AND 200000))
    );
    CREATE TRIGGER IF NOT EXISTS album_cover_photo_moved AFTER UPDATE OF album_id ON photos
    WHEN OLD.album_id != NEW.album_id BEGIN
        DELETE FROM album_covers WHERE photo_id=NEW.id AND album_id!=NEW.album_id;
    END;
    CREATE TRIGGER IF NOT EXISTS album_cover_insert_membership BEFORE INSERT ON album_covers
    WHEN NEW.photo_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM photos WHERE id=NEW.photo_id AND album_id=NEW.album_id)
    BEGIN SELECT RAISE(ABORT,'cover photo must belong to album'); END;
    CREATE TRIGGER IF NOT EXISTS album_cover_update_membership BEFORE UPDATE ON album_covers
    WHEN NEW.photo_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM photos WHERE id=NEW.photo_id AND album_id=NEW.album_id)
    BEGIN SELECT RAISE(ABORT,'cover photo must belong to album'); END;")
        .execute(db).await?;
    Ok(())
}

fn not_found() -> AppError {
    AppError {
        status: StatusCode::NOT_FOUND,
        message: "相册或封面不存在".into(),
        clear_auth_cookies: None,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Cover {
    pub cover_source: &'static str,
    pub cover_photo_id: Option<String>,
    pub cover_url: Option<String>,
}

pub(super) fn from_row(row: &sqlx::sqlite::SqliteRow) -> Cover {
    let photo_id: Option<String> = row.get("cover_photo_id");
    let version: Option<String> = row.get("cover_version");
    let first_photo_id: Option<String> = row.get("first_photo_id");
    let cover_source = if photo_id.is_some() {
        "photo"
    } else if version.is_some() {
        "upload"
    } else {
        "auto"
    };
    let cover_url = if let Some(id) = photo_id
        .as_ref()
        .or(first_photo_id.as_ref().filter(|_| version.is_none()))
    {
        Some(format!(
            "/api/photos/{}/thumbnail?v=grid2",
            urlencoding::encode(id)
        ))
    } else {
        version.map(|version| {
            format!(
                "/api/albums/{}/cover/{}",
                urlencoding::encode(row.get::<&str, _>("id")),
                urlencoding::encode(&version)
            )
        })
    };
    Cover {
        cover_source,
        cover_photo_id: photo_id,
        cover_url,
    }
}

async fn replace(
    state: &AppState,
    album_id: &str,
    photo_id: Option<&str>,
    image: Option<Vec<u8>>,
) -> ApiResult<Cover> {
    // Serialize validation and replacement with deletion/moving, including other clients.
    let mut tx = state
        .db
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(AppError::internal)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM albums WHERE id=?)")
        .bind(album_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    if !exists {
        return Err(not_found());
    }
    if let Some(photo_id) = photo_id {
        let belongs: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM photos WHERE id=? AND album_id=?)")
                .bind(photo_id)
                .bind(album_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(AppError::internal)?;
        if !belongs {
            return Err(AppError::bad(
                "请选择当前相册内的图片；图片可能已经被删除或移动",
            ));
        }
    }
    if photo_id.is_none() && image.is_none() {
        sqlx::query("DELETE FROM album_covers WHERE album_id=?")
            .bind(album_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::internal)?;
    } else {
        sqlx::query("INSERT INTO album_covers(album_id,photo_id,image,version) VALUES(?,?,?,?) ON CONFLICT(album_id) DO UPDATE SET photo_id=excluded.photo_id,image=excluded.image,version=excluded.version")
            .bind(album_id).bind(photo_id).bind(image).bind(Uuid::new_v4().to_string())
            .execute(&mut *tx).await.map_err(AppError::internal)?;
    }
    let row = sqlx::query(&format!("{ALBUM_SELECT} WHERE a.id=? GROUP BY a.id"))
        .bind(album_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::internal)?;
    let cover = from_row(&row);
    tx.commit().await.map_err(AppError::internal)?;
    Ok(cover)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Selection {
    photo_id: String,
}

pub(super) async fn select(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    Json(input): Json<Selection>,
) -> ApiResult<Json<Cover>> {
    require_admin(&headers, &state, true).await?;
    Ok(Json(
        replace(&state, &id, Some(&input.photo_id), None).await?,
    ))
}

pub(super) async fn reset(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> ApiResult<Json<Cover>> {
    require_admin(&headers, &state, true).await?;
    Ok(Json(replace(&state, &id, None, None).await?))
}

fn encode_cover(path: &Path, filename: &str) -> Result<Vec<u8>> {
    let expected = file_format(filename).context("封面仅支持 PNG、JPG/JPEG、WebP")?;
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let detected = match reader.format() {
        Some(ImageFormat::Png) => "png",
        Some(ImageFormat::Jpeg) => "jpg",
        Some(ImageFormat::WebP) => "webp",
        _ => bail!("文件不是有效的 PNG、JPG/JPEG 或 WebP 图片"),
    };
    if detected != expected {
        bail!("封面扩展名与实际图片格式不一致");
    }
    // ImageReader's decoder allocation limits also protect against decompression bombs.
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    encode_limited_webp_from_image(&image, COVER_EDGE, COVER_MAX_BYTES)
}

pub(super) async fn upload(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult<Json<Cover>> {
    require_admin(&headers, &state, true).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM albums WHERE id=?)")
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(AppError::internal)?;
    if !exists {
        return Err(not_found());
    }
    let permit = state
        .upload_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(AppError::internal)?;
    let directory = std::env::temp_dir().join(format!("chronoframe-cover-{}", Uuid::new_v4()));
    tokio::fs::create_dir(&directory)
        .await
        .map_err(AppError::internal)?;
    let guard = ExportTempGuard::new(directory.clone());
    let path = directory.join("upload");
    let mut field = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad(e.to_string()))?
        .ok_or_else(|| AppError::bad("请选择一张封面图片"))?;
    let filename = field.file_name().unwrap_or_default().to_string();
    if field.name() != Some("file") || file_format(&filename).is_none() {
        return Err(AppError::bad("请选择一张 PNG、JPG/JPEG 或 WebP 封面"));
    }
    // Stream the source to a private temporary file; never buffer an unbounded upload.
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(AppError::internal)?;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|e| AppError::bad(e.to_string()))?
    {
        file.write_all(&chunk).await.map_err(AppError::internal)?;
    }
    file.flush().await.map_err(AppError::internal)?;
    drop(file);
    drop(field);
    if multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad(e.to_string()))?
        .is_some()
    {
        return Err(AppError::bad("一次只能上传一张封面"));
    }
    let image = tokio::task::spawn_blocking(move || {
        // These guards remain alive if the HTTP request is cancelled during encoding.
        let (_permit, _guard) = (permit, guard);
        encode_cover(&path, &filename)
    })
    .await
    .map_err(AppError::internal)?
    .map_err(|e| AppError::bad(format!("封面处理失败：{e}")))?;
    Ok(Json(replace(&state, &id, None, Some(image)).await?))
}

pub(super) async fn serve(
    State(state): State<AppState>,
    AxumPath((id, version)): AxumPath<(String, String)>,
) -> ApiResult<Response> {
    let image: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT image FROM album_covers WHERE album_id=? AND version=? AND image IS NOT NULL",
    )
    .bind(id)
    .bind(version)
    .fetch_optional(&state.db)
    .await
    .map_err(AppError::internal)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/webp"),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        image.ok_or_else(not_found)?,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn fixture() -> AppState {
        let state = crate::tests::test_state().await;
        sqlx::raw_sql("INSERT INTO albums(id,name,created_at) VALUES('a','Album A',1),('b','Album B',2);
            INSERT INTO photos(id,album_id,original_name,storage_key,format,content_type,byte_size,created_at) VALUES
            ('p1','a','1.png','1.png','png','image/png',10,1),
            ('p2','a','2.png','2.png','png','image/png',20,2),
            ('p3','b','3.png','3.png','png','image/png',30,3);")
            .execute(&state.db).await.unwrap();
        state
    }

    async fn album(state: &AppState, id: &str) -> Album {
        let row = sqlx::query(&format!("{ALBUM_SELECT} WHERE a.id=? GROUP BY a.id"))
            .bind(id)
            .fetch_one(&state.db)
            .await
            .unwrap();
        album_from(&row)
    }

    #[tokio::test]
    async fn selection_is_album_scoped_and_reset_does_not_delete_photos() {
        let state = fixture().await;
        assert_eq!(
            album(&state, "a").await.cover.cover_url.as_deref(),
            Some("/api/photos/p2/thumbnail?v=grid2")
        );
        let selected = replace(&state, "a", Some("p1"), None).await.unwrap();
        assert_eq!(selected.cover_source, "photo");
        assert_eq!(selected.cover_photo_id.as_deref(), Some("p1"));
        assert!(replace(&state, "a", Some("p3"), None).await.is_err());
        assert!(replace(&state, "a", Some("missing"), None).await.is_err());
        assert!(replace(&state, "missing", Some("p1"), None).await.is_err());
        assert_eq!(
            album(&state, "a").await.cover.cover_photo_id.as_deref(),
            Some("p1")
        );
        let reset = replace(&state, "a", None, None).await.unwrap();
        assert_eq!(reset.cover_source, "auto");
        assert_eq!(album(&state, "a").await.photo_count, 2);
        replace(&state, "a", None, None).await.unwrap();
    }

    #[tokio::test]
    async fn uploaded_bytes_are_atomic_versioned_and_removed_on_replacement() {
        let state = fixture().await;
        let first = replace(&state, "a", None, Some(vec![1, 2, 3]))
            .await
            .unwrap();
        let first_version = first
            .cover_url
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();
        let second = replace(&state, "a", None, Some(vec![4, 5])).await.unwrap();
        assert_eq!(second.cover_source, "upload");
        assert!(!second.cover_url.unwrap().ends_with(&first_version));
        assert!(
            serve(State(state.clone()), AxumPath(("a".into(), first_version)))
                .await
                .is_err()
        );
        let bytes: Vec<u8> =
            sqlx::query_scalar("SELECT image FROM album_covers WHERE album_id='a'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(bytes, vec![4, 5]);
        replace(&state, "a", Some("p1"), None).await.unwrap();
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM album_covers WHERE image IS NOT NULL")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(count, 0);
        assert_eq!(album(&state, "a").await.photo_count, 2);
    }

    #[tokio::test]
    async fn deleted_or_moved_cover_photo_falls_back_and_album_cascades() {
        let state = fixture().await;
        replace(&state, "a", Some("p1"), None).await.unwrap();
        sqlx::query("UPDATE photos SET album_id='b' WHERE id='p1'")
            .execute(&state.db)
            .await
            .unwrap();
        assert_eq!(album(&state, "a").await.cover.cover_source, "auto");
        replace(&state, "a", Some("p2"), None).await.unwrap();
        sqlx::query("DELETE FROM photos WHERE id='p2'")
            .execute(&state.db)
            .await
            .unwrap();
        assert!(album(&state, "a").await.cover.cover_url.is_none());
        replace(&state, "a", None, Some(vec![1])).await.unwrap();
        assert_eq!(album(&state, "a").await.photo_count, 0);
        sqlx::query("DELETE FROM albums WHERE id='a'")
            .execute(&state.db)
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM album_covers")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn covers_do_not_invalidate_downloads_and_migration_is_idempotent() {
        let state = fixture().await;
        sqlx::query("INSERT INTO album_download_settings(album_id,enabled,formats,max_image_bytes,max_zip_bytes,revision,updated_at) VALUES('a',1,'[\"webp\"]',5000000,0,7,1)").execute(&state.db).await.unwrap();
        let (one, two) = tokio::join!(
            replace(&state, "a", Some("p1"), None),
            replace(&state, "a", None, Some(vec![1, 2]))
        );
        one.unwrap();
        two.unwrap();
        setup(&state.db).await.unwrap();
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM album_download_settings WHERE album_id='a'")
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(revision, 7);
        assert_eq!(album(&state, "a").await.photo_count, 2);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM album_covers")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn encoding_validates_content_and_bounds_stored_size() {
        let directory =
            std::env::temp_dir().join(format!("chronoframe-cover-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let _guard = ExportTempGuard::new(directory.clone());
        let path = directory.join("input");
        for (format, filename) in [
            (ImageFormat::Png, "test.png"),
            (ImageFormat::Jpeg, "test.jpeg"),
            (ImageFormat::WebP, "test.webp"),
        ] {
            let mut output = Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(1600, 900)
                .write_to(&mut output, format)
                .unwrap();
            std::fs::write(&path, output.into_inner()).unwrap();
            let cover = encode_cover(&path, filename).unwrap();
            assert!(cover.len() <= COVER_MAX_BYTES);
            assert_eq!(image::guess_format(&cover).unwrap(), ImageFormat::WebP);
            let decoded = image::load_from_memory(&cover).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (800, 450));
        }
        assert!(encode_cover(&path, "wrong.png").is_err());
        std::fs::write(&path, b"<svg></svg>").unwrap();
        assert!(encode_cover(&path, "fake.png").is_err());
    }
}
