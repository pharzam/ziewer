//! Embedded static assets under `/_assets/`.

use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use http::{StatusCode, header};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

pub async fn asset(Path(file): Path<String>) -> Response {
    match Assets::get(&file) {
        Some(content) => {
            let mime = mime_guess::from_path(&file).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, "public, max-age=3600".into())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}
