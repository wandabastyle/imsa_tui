use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

#[derive(Clone)]
pub struct StaticConfig {
    pub root_dir: PathBuf,
}

pub async fn index(config: StaticConfig) -> impl IntoResponse {
    serve_file_or_404(config.root_dir.join("index.html")).await
}

pub async fn asset_or_index(config: StaticConfig, request_path: &str) -> impl IntoResponse {
    let Some(clean_path) = clean_relative_path(request_path) else {
        return StatusCode::BAD_REQUEST.into_response();
    };

    let candidate = config.root_dir.join(clean_path);
    if candidate.is_file() {
        return serve_file_or_404(candidate).await;
    }

    // SPA fallback so deep links are handled by SvelteKit client routing.
    serve_file_or_404(config.root_dir.join("index.html")).await
}

async fn serve_file_or_404(path: PathBuf) -> Response {
    let data = match tokio::fs::read(&path).await {
        Ok(data) => data,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    let mut response = Response::new(Body::from(data));
    *response.status_mut() = StatusCode::OK;
    if let Ok(content_type) = HeaderValue::from_str(mime.as_ref()) {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    response
}

fn clean_relative_path(request_path: &str) -> Option<PathBuf> {
    let trimmed = request_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some(PathBuf::from("index.html"));
    }

    let candidate = Path::new(trimmed);
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }

    Some(candidate.to_path_buf())
}
