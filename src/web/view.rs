//! Folder browsing and file rendering.

use super::{AppState, base_ctx, error_page, render};
use crate::access::Role;
use crate::fs_tree::{TreeNode, build_tree, dir_index, list_dir, resolve};
use crate::render::{inject_mermaid, markdown_to_html};
use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use http::{StatusCode, header};
use minijinja::context;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use std::path::{Path as FsPath, PathBuf};

/// Characters left unescaped in a path segment.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[derive(Deserialize)]
pub struct ViewQuery {
    raw: Option<String>,
}

#[derive(Serialize)]
struct NavNode {
    name: String,
    href: String,
    is_dir: bool,
    active: bool,
    open: bool,
    children: Vec<NavNode>,
}

fn encode_rel(rel: &str) -> String {
    rel.split('/').map(|s| utf8_percent_encode(s, SEGMENT).to_string()).collect::<Vec<_>>().join("/")
}

fn href(slug: &str, rel: &str, is_dir: bool) -> String {
    let mut h = format!("/f/{slug}/{}", encode_rel(rel));
    if is_dir && !rel.is_empty() {
        h.push('/');
    }
    h
}

fn nav(slug: &str, nodes: Vec<TreeNode>, current: &str) -> Vec<NavNode> {
    nodes
        .into_iter()
        .map(|n| NavNode {
            href: href(slug, &n.rel_path, n.is_dir),
            active: n.rel_path == current,
            open: n.is_dir && (current == n.rel_path || current.starts_with(&format!("{}/", n.rel_path))),
            is_dir: n.is_dir,
            name: n.name,
            children: nav(slug, n.children, current),
        })
        .collect()
}

pub async fn root(State(state): State<AppState>, Extension(role): Extension<Role>) -> Response {
    let first = state.config.read().unwrap().effective().folders.first().map(|f| f.slug());
    match first {
        Some(slug) => (StatusCode::FOUND, [(header::LOCATION, format!("/f/{slug}/"))]).into_response(),
        None => error_page(&state, role, StatusCode::NOT_FOUND, "No folders", "No folders configured."),
    }
}

pub async fn folder_no_slash(Path(slug): Path<String>) -> Response {
    Redirect::permanent(&format!("/f/{}/", utf8_percent_encode(&slug, SEGMENT))).into_response()
}

pub async fn folder_root(
    State(state): State<AppState>,
    Extension(role): Extension<Role>,
    Path(slug): Path<String>,
    Query(q): Query<ViewQuery>,
) -> Response {
    view(&state, role, &slug, "", true, q.raw.is_some()).await
}

pub async fn folder_path(
    State(state): State<AppState>,
    Extension(role): Extension<Role>,
    Path((slug, path)): Path<(String, String)>,
    uri: http::Uri,
    Query(q): Query<ViewQuery>,
) -> Response {
    let trailing = uri.path().ends_with('/');
    view(&state, role, &slug, path.trim_end_matches('/'), trailing, q.raw.is_some()).await
}

fn ext_lower(p: &FsPath) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// What a request resolves to, computed off the async runtime (filesystem walks block).
enum Outcome {
    NotFound,
    Redirect(String),
    Raw(PathBuf, String),
    Listing { tree: Vec<NavNode>, entries: Vec<NavNode>, title: String },
    Page { file: PathBuf, tree: Vec<NavNode>, title: String },
}

fn resolve_outcome(root: Option<PathBuf>, slug: &str, rel: &str, trailing: bool, raw: bool) -> Outcome {
    let Some(root) = root.and_then(|r| r.canonicalize().ok()) else { return Outcome::NotFound };
    let Some(target) = resolve(&root, rel) else { return Outcome::NotFound };

    if target.is_dir() {
        if !trailing {
            return Outcome::Redirect(format!("{}/", href(slug, rel, false)));
        }
        let tree = nav(slug, build_tree(&root), rel);
        let title = if rel.is_empty() { slug.to_string() } else { rel.to_string() };
        return match dir_index(&target) {
            Some(file) if ext_lower(&file) == "html" => Outcome::Raw(file, "html".into()),
            Some(file) => Outcome::Page { file, tree, title },
            None => Outcome::Listing { entries: nav(slug, list_dir(&root, &target), ""), tree, title },
        };
    }

    let ext = ext_lower(&target);
    // HTML files open as themselves (full page, own styles/scripts); only Markdown is wrapped.
    if raw || ext != "md" {
        return Outcome::Raw(target, ext);
    }
    let tree = nav(slug, build_tree(&root), rel);
    let title = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Outcome::Page { file: target, tree, title }
}

async fn view(state: &AppState, role: Role, slug: &str, rel: &str, trailing: bool, raw: bool) -> Response {
    let root: Option<PathBuf> = state.config.read().unwrap().effective().folder_by_slug(slug).map(|f| f.path.clone());
    let (slug_owned, rel_owned) = (slug.to_string(), rel.to_string());
    let outcome = tokio::task::spawn_blocking(move || resolve_outcome(root, &slug_owned, &rel_owned, trailing, raw))
        .await
        .unwrap_or(Outcome::NotFound);
    match outcome {
        Outcome::NotFound => error_page(state, role, StatusCode::NOT_FOUND, "Not found", "That file does not exist."),
        Outcome::Redirect(to) => Redirect::permanent(&to).into_response(),
        Outcome::Raw(path, ext) => raw_file(&path, &ext).await,
        Outcome::Listing { tree, entries, title } => {
            let ctx = context! { mode => "listing", title, tree, entries, ..base_ctx(state, role, Some(slug)) };
            render(state, "view.html", ctx)
        }
        Outcome::Page { file, tree, title } => page(state, role, slug, &file, tree, &title).await,
    }
}

/// Render a Markdown file inside the layout.
async fn page(
    state: &AppState,
    role: Role,
    slug: &str,
    file: &FsPath,
    tree: Vec<NavNode>,
    title: &str,
) -> Response {
    let base = base_ctx(state, role, Some(slug));
    match tokio::fs::read(file).await {
        Ok(bytes) => {
            let content = markdown_to_html(&String::from_utf8_lossy(&bytes));
            render(state, "view.html", context! { mode => "markdown", title, tree, content, ..base })
        }
        Err(_) => error_page(state, role, StatusCode::NOT_FOUND, "Not found", "That file could not be read."),
    }
}

/// Largest Markdown file accepted by `POST /render`.
pub const MAX_DROP_BYTES: usize = 5 * 1024 * 1024;

/// Render Markdown sent by the browser (a file dropped on the page) and return the HTML
/// fragment. Nothing is written to disk.
pub async fn render_dropped(body: axum::body::Bytes) -> Response {
    let Ok(text) = String::from_utf8(body.to_vec()) else {
        return (StatusCode::BAD_REQUEST, "file is not UTF-8 text").into_response();
    };
    match tokio::task::spawn_blocking(move || markdown_to_html(&text)).await {
        Ok(html) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "render failed").into_response(),
    }
}

async fn raw_file(path: &FsPath, ext: &str) -> Response {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    match ext {
        "html" => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            inject_mermaid(&String::from_utf8_lossy(&bytes)),
        )
            .into_response(),
        "md" => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], bytes).into_response(),
        _ => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref().to_string())], bytes).into_response()
        }
    }
}
