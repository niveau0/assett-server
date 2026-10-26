use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    Json,
    body::Body,
    extract::{Extension, Multipart, Path, State, multipart::Field},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

use crate::{
    AppState,
    auth::Auth,
    metadata::{FileEntry, Project, hash_key},
};

// ─── Error ────────────────────────────────────────────────────────────────────

pub enum AppError {
    NotFound,
    Unauthorized,
    BadRequest(String),
    Conflict(String),
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found\n").into_response(),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized\n").into_response(),
            AppError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, format!("{msg}\n")).into_response()
            }
            AppError::Conflict(msg) => (StatusCode::CONFLICT, format!("{msg}\n")).into_response(),
            AppError::Internal(err) => {
                tracing::error!("internal error: {err:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error\n").into_response()
            }
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Internal(err.into())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Internal(err)
    }
}

impl From<axum::extract::multipart::MultipartError> for AppError {
    fn from(err: axum::extract::multipart::MultipartError) -> Self {
        use std::error::Error;
        let detail = err.source().map(|s| format!(": {s}")).unwrap_or_default();
        AppError::BadRequest(format!("{err}{detail}"))
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn validate_segment(segment: &str) -> Result<(), AppError> {
    if segment.is_empty()
        || segment == "."
        || segment.contains("..")
        || segment.contains('/')
        || segment.contains('\\')
        || segment.contains('\0')
        || segment.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest("invalid path segment".into()));
    }
    Ok(())
}

fn validate_project_name(name: &str) -> Result<(), AppError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

    if valid {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "project name must be 1-64 chars of [A-Za-z0-9_-]".into(),
        ))
    }
}

fn authorize_project(auth: &Auth, project: &str) -> Result<(), AppError> {
    match auth {
        Auth::Admin => Ok(()),
        Auth::Project(name) if name == project => Ok(()),
        _ => Err(AppError::Unauthorized),
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn content_type(filename: &str) -> &'static str {
    match filename.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ktx" | "ktx2" => "image/ktx2",
        "json" => "application/json",
        "glb" => "model/gltf-binary",
        "gltf" => "model/gltf+json",
        "mp4" => "video/mp4",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "wasm" => "application/wasm",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "7z" => "application/x-7z-compressed",
        "exe" | "dll" => "application/vnd.microsoft.portable-executable",
        "apk" => "application/vnd.android.package-archive",
        "dmg" => "application/x-apple-diskimage",
        _ => "application/octet-stream",
    }
}

/// Removes a partially written temp file when dropped, unless `keep` is set.
struct TempUpload {
    path: PathBuf,
    filename: String,
    size: u64,
    keep: bool,
}

impl Drop for TempUpload {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

static UPLOAD_COUNTER: AtomicU64 = AtomicU64::new(0);

async fn stream_field_to_disk(
    field: &mut Field<'_>,
    path: &std::path::Path,
) -> Result<u64, AppError> {
    let mut file = tokio::fs::File::create(path).await?;
    let mut size = 0u64;

    while let Some(chunk) = field.chunk().await? {
        file.write_all(&chunk).await?;
        size += chunk.len() as u64;
    }

    file.flush().await?;
    Ok(size)
}

// ─── Project management ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateProject {
    pub name: String,
    pub api_key: String,
}

/// POST /assets
/// Admin only. Body: { "name": "...", "api_key": "..." }
pub async fn create_project(
    State(state): State<AppState>,
    Extension(auth): Extension<Auth>,
    Json(payload): Json<CreateProject>,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    if !matches!(auth, Auth::Admin) {
        return Err(AppError::Unauthorized);
    }

    validate_project_name(&payload.name)?;
    if payload.api_key.is_empty() {
        return Err(AppError::BadRequest("api_key must not be empty".into()));
    }

    let mut metadata = state.metadata.write().await;
    if metadata.projects.contains_key(&payload.name) {
        return Err(AppError::Conflict("project already exists".into()));
    }

    tokio::fs::create_dir_all(state.config.storage_path.join(&payload.name)).await?;

    metadata.projects.insert(
        payload.name.clone(),
        Project {
            api_key_sha256: hash_key(&payload.api_key),
            files: Default::default(),
        },
    );
    metadata.save(&state.config.storage_path).await?;

    tracing::info!(project = %payload.name, "project created");

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "name": payload.name })),
    ))
}

#[derive(Deserialize)]
pub struct UpdateProject {
    pub api_key: String,
}

/// PUT /assets/{project}
/// Admin or the project's own key. Replaces the project API key.
pub async fn update_project(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Extension(auth): Extension<Auth>,
    Json(payload): Json<UpdateProject>,
) -> Result<StatusCode, AppError> {
    validate_segment(&project)?;
    authorize_project(&auth, &project)?;

    if payload.api_key.is_empty() {
        return Err(AppError::BadRequest("api_key must not be empty".into()));
    }

    let mut metadata = state.metadata.write().await;
    let entry = metadata
        .projects
        .get_mut(&project)
        .ok_or(AppError::NotFound)?;
    entry.api_key_sha256 = hash_key(&payload.api_key);
    metadata.save(&state.config.storage_path).await?;

    tracing::info!(project = %project, "project api key updated");
    Ok(StatusCode::NO_CONTENT)
}

// ─── Assets ───────────────────────────────────────────────────────────────────

/// POST /assets/{project}
/// Multipart: exactly one `file`, optional `public` (true/false/1/0, default false).
pub async fn upload(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Extension(auth): Extension<Auth>,
    mut multipart: Multipart,
) -> Result<StatusCode, AppError> {
    validate_segment(&project)?;
    authorize_project(&auth, &project)?;

    {
        let metadata = state.metadata.read().await;
        if !metadata.projects.contains_key(&project) {
            return Err(AppError::NotFound);
        }
    }

    let project_dir = state.config.storage_path.join(&project);
    tokio::fs::create_dir_all(&project_dir).await?;
    let tmp_dir = state.config.storage_path.join(".tmp");

    let mut public = false;
    let mut pending: Option<TempUpload> = None;

    while let Some(mut field) = multipart.next_field().await? {
        let field_name = field.name().map(str::to_owned);

        match field_name.as_deref() {
            Some("public") => {
                let value = field.text().await?;
                public = parse_bool(&value).ok_or_else(|| {
                    AppError::BadRequest("public must be one of true/false/1/0".into())
                })?;
            }
            Some("file") => {
                if pending.is_some() {
                    return Err(AppError::BadRequest(
                        "only one file per request is allowed".into(),
                    ));
                }

                let filename = field
                    .file_name()
                    .ok_or_else(|| AppError::BadRequest("file field missing filename".into()))?
                    .to_string();
                validate_segment(&filename)?;

                let tmp_path = tmp_dir.join(format!(
                    "upload-{}-{}-{}",
                    std::process::id(),
                    UPLOAD_COUNTER.fetch_add(1, Ordering::Relaxed),
                    Utc::now().timestamp_nanos_opt().unwrap_or_default()
                ));

                let mut upload = TempUpload {
                    path: tmp_path,
                    filename,
                    size: 0,
                    keep: false,
                };
                upload.size = stream_field_to_disk(&mut field, &upload.path).await?;
                pending = Some(upload);
            }
            _ => {}
        }
    }

    let mut upload = pending
        .ok_or_else(|| AppError::BadRequest("no file field found in multipart body".into()))?;

    let destination = project_dir.join(&upload.filename);
    tokio::fs::rename(&upload.path, &destination).await?;
    upload.keep = true;

    let filename = upload.filename.clone();
    let size = upload.size;
    let modified_at = Utc::now();

    {
        let mut metadata = state.metadata.write().await;
        let project_entry = metadata
            .projects
            .get_mut(&project)
            .ok_or(AppError::NotFound)?;
        project_entry.files.insert(
            filename.clone(),
            FileEntry {
                public,
                size,
                modified_at,
            },
        );
        metadata.save(&state.config.storage_path).await?;
    }

    tracing::info!(
        project = %project,
        file = %filename,
        bytes = size,
        public,
        "uploaded"
    );

    Ok(StatusCode::OK)
}

#[derive(Serialize)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub modified_at: DateTime<Utc>,
    pub public: bool,
}

/// GET /assets/{project}
/// Anonymous sees public files only; admin or the project's own key sees all.
pub async fn metadata(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Extension(auth): Extension<Auth>,
) -> Result<Json<Vec<FileInfo>>, AppError> {
    validate_segment(&project)?;

    if let Auth::Project(name) = &auth
        && name != &project
    {
        return Err(AppError::Unauthorized);
    }

    let is_anonymous = matches!(&auth, Auth::Anonymous);

    let metadata = state.metadata.read().await;
    let Some(project_entry) = metadata.projects.get(&project) else {
        return if is_anonymous {
            Ok(Json(Vec::new()))
        } else {
            Err(AppError::NotFound)
        };
    };

    let mut files: Vec<FileInfo> = project_entry
        .files
        .iter()
        .filter(|(_, file)| !is_anonymous || file.public)
        .map(|(name, file)| FileInfo {
            name: name.clone(),
            size: file.size,
            modified_at: file.modified_at,
            public: file.public,
        })
        .collect();

    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(files))
}

/// GET /assets/{project}/{filename}
/// Public files work anonymously; private files require admin or the project's key.
pub async fn download(
    State(state): State<AppState>,
    Path((project, filename)): Path<(String, String)>,
    Extension(auth): Extension<Auth>,
) -> Result<Response, AppError> {
    validate_segment(&project)?;
    validate_segment(&filename)?;

    if let Auth::Project(name) = &auth
        && name != &project
    {
        return Err(AppError::Unauthorized);
    }

    let is_anonymous = matches!(&auth, Auth::Anonymous);

    {
        let metadata = state.metadata.read().await;
        let project_entry = metadata.projects.get(&project).ok_or(AppError::NotFound)?;
        let file_entry = project_entry
            .files
            .get(&filename)
            .ok_or(AppError::NotFound)?;

        if is_anonymous && !file_entry.public {
            return Err(AppError::NotFound);
        }
    }

    let path = state.config.storage_path.join(&project).join(&filename);
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::NotFound);
        }
        Err(err) => return Err(AppError::Internal(err.into())),
    };

    let file_meta = file.metadata().await?;
    let mime = content_type(&filename);

    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, file_meta.len());

    if mime == "application/octet-stream" {
        let encoded = utf8_percent_encode(&filename, NON_ALPHANUMERIC);
        builder = builder.header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename*=UTF-8''{encoded}"),
        );
    }

    tracing::info!(
        project = %project,
        file = %filename,
        bytes = file_meta.len(),
        "downloaded"
    );

    Ok(builder
        .body(Body::from_stream(ReaderStream::new(file)))
        .expect("valid response"))
}
