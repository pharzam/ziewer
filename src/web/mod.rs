//! HTTP layer: shared state, router, middleware.

mod admin;
mod assets;
mod gate;
mod view;

use crate::access::{Role, classify, client_ip};
use crate::config::{ConfigStore, slugify};
use crate::requests::RequestQueue;
use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use http::{HeaderName, HeaderValue, StatusCode};
use minijinja::{Environment, Value, context};
use rust_embed::RustEmbed;
use serde::Serialize;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, RwLock};
use tower_http::set_header::SetResponseHeaderLayer;

#[derive(RustEmbed)]
#[folder = "templates/"]
struct Templates;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<ConfigStore>>,
    pub requests: Arc<Mutex<RequestQueue>>,
    pub templates: Arc<Environment<'static>>,
}

/// The resolved client IP, inserted into request extensions by the access middleware.
#[derive(Clone, Copy, Debug)]
pub struct ClientIp(pub IpAddr);

pub fn build_state(store: ConfigStore) -> io::Result<AppState> {
    let req_path = store.path().parent().unwrap_or(std::path::Path::new(".")).join("requests.json");
    let requests = RequestQueue::load(req_path)?;
    let mut env = Environment::new();
    env.set_formatter(html_formatter);
    for name in Templates::iter() {
        let file = Templates::get(&name).expect("listed template");
        let src = String::from_utf8(file.data.into_owned())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        env.add_template_owned(name.to_string(), src)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    }
    Ok(AppState {
        config: Arc::new(RwLock::new(store)),
        requests: Arc::new(Mutex::new(requests)),
        templates: Arc::new(env),
    })
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(view::root))
        .route("/f/{slug}", get(view::folder_no_slash))
        .route("/f/{slug}/", get(view::folder_root))
        .route("/f/{slug}/{*path}", get(view::folder_path))
        .route("/_assets/{*file}", get(assets::asset))
        .route("/request-access", post(gate::request_access))
        .route(
            "/render",
            post(view::render_dropped).layer(axum::extract::DefaultBodyLimit::max(view::MAX_DROP_BYTES)),
        )
        .route("/admin", get(admin::dashboard))
        .route("/admin/requests/{ip}/approve", post(admin::approve))
        .route("/admin/requests/{ip}/deny", post(admin::deny))
        .route("/admin/ips", post(admin::add_ip))
        .route("/admin/ips/delete", post(admin::delete_ip))
        .route("/admin/folders", post(admin::add_folder))
        .route("/admin/folders/delete", post(admin::delete_folder))
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(state.clone(), access_mw))
        .layer(header("x-content-type-options", "nosniff"))
        .layer(header("referrer-policy", "same-origin"))
        .layer(header("x-frame-options", "SAMEORIGIN"))
        .with_state(state)
}

fn header(name: &'static str, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(HeaderName::from_static(name), HeaderValue::from_static(value))
}

pub async fn serve(state: AppState) -> io::Result<()> {
    let listen = {
        let store = state.config.read().unwrap();
        for f in &store.effective().folders {
            if let Err(e) = f.check_exists() {
                tracing::warn!("{e} (folder '{}' will return 404 until it exists)", f.name);
            }
        }
        store.effective().listen.clone()
    };
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!("ziewer listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router(state).into_make_service_with_connect_info::<SocketAddr>()).await
}

/// Resolve client IP + role and stash both in request extensions.
async fn access_mw(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0)
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 0)));
    let host = req.headers().get(http::header::HOST).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let (ip, role, host_ok) = {
        let store = state.config.read().unwrap();
        let cfg = store.effective();
        let ip = client_ip(peer, req.headers(), cfg.trust_proxy);
        (ip, classify(ip, cfg), host.as_deref().is_none_or(|h| cfg.host_allowed(h)))
    };
    if !host_ok {
        tracing::warn!("rejected request for unknown host {host:?} from {ip}");
        return (
            StatusCode::MISDIRECTED_REQUEST,
            "Unknown host name. Add it to `hostnames` in the Ziewer config.",
        )
            .into_response();
    }
    req.extensions_mut().insert(ClientIp(ip));
    req.extensions_mut().insert(role);

    let path = req.uri().path();
    if req.method() == http::Method::POST && gate::origin_mismatch(req.headers()) {
        return error_page(&state, role, StatusCode::FORBIDDEN, "Bad origin", "Cross-site form submissions are not allowed.");
    }
    if path.starts_with("/_assets/") {
        return next.run(req).await;
    }
    let is_admin_path = path == "/admin" || path.starts_with("/admin/");
    if is_admin_path && role != Role::Admin {
        if role == Role::Denied {
            return gate::denied(&state, ip);
        }
        return error_page(&state, role, StatusCode::FORBIDDEN, "Admin only", "This page is only available to the admin.");
    }
    if role == Role::Denied && !(req.method() == http::Method::POST && path == "/request-access") {
        return gate::denied(&state, ip);
    }
    next.run(req).await
}

#[derive(Serialize)]
struct MenuItem {
    name: String,
    slug: String,
    active: bool,
}

/// Context shared by every page that extends `base.html`.
pub(crate) fn base_ctx(state: &AppState, role: Role, active: Option<&str>) -> Value {
    let store = state.config.read().unwrap();
    let folders: Vec<MenuItem> = if role == Role::Denied {
        vec![]
    } else {
        store
            .effective()
            .folders
            .iter()
            .map(|f| {
                let slug = slugify(&f.name);
                MenuItem { active: Some(slug.as_str()) == active, name: f.name.clone(), slug }
            })
            .collect()
    };
    context! { folders, is_admin => role == Role::Admin }
}

/// Like minijinja's default, but escapes only `& < > " '` (keeps `/` readable in URLs).
fn html_formatter(out: &mut minijinja::Output, state: &minijinja::State, value: &Value) -> Result<(), minijinja::Error> {
    let plain = !value.is_safe() && !value.is_undefined() && !value.is_none();
    if plain && matches!(state.auto_escape(), minijinja::AutoEscape::Html) {
        let s = value.to_string();
        let mut esc = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => esc.push_str("&amp;"),
                '<' => esc.push_str("&lt;"),
                '>' => esc.push_str("&gt;"),
                '"' => esc.push_str("&quot;"),
                '\'' => esc.push_str("&#39;"),
                _ => esc.push(c),
            }
        }
        out.write_str(&esc).map_err(|e| minijinja::Error::new(minijinja::ErrorKind::WriteFailure, e.to_string()))
    } else {
        minijinja::escape_formatter(out, state, value)
    }
}

pub(crate) fn render(state: &AppState, tpl: &str, ctx: Value) -> Response {
    match state.templates.get_template(tpl).and_then(|t| t.render(ctx)) {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("template {tpl}: {e:#}");
            (StatusCode::INTERNAL_SERVER_ERROR, "template error").into_response()
        }
    }
}

pub(crate) fn error_page(state: &AppState, role: Role, status: StatusCode, title: &str, message: &str) -> Response {
    let ctx = context! { title, message, ..base_ctx(state, role, None) };
    (status, render(state, "error.html", ctx)).into_response()
}

async fn not_found(State(state): State<AppState>, req: Request) -> Response {
    let role = req.extensions().get::<Role>().copied().unwrap_or(Role::Denied);
    error_page(&state, role, StatusCode::NOT_FOUND, "Not found", "That page does not exist.")
}
