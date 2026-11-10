#![allow(dead_code)]

use axum::body::Body;
use axum::extract::ConnectInfo;
use http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use std::fs;
use std::net::SocketAddr;
use tempfile::TempDir;
use tower::ServiceExt;
use ziewer::config::ConfigStore;
use ziewer::web::{AppState, build_state, router};

pub const ADMIN: &str = "10.0.0.1";
pub const ALLOWED: &str = "10.0.0.2";
pub const STRANGER: &str = "8.8.8.8";

pub struct Fixture {
    pub dir: TempDir,
    pub state: AppState,
}

impl Fixture {
    pub fn config_path(&self) -> std::path::PathBuf {
        self.dir.path().join("ziewer.toml")
    }
    pub fn config_text(&self) -> String {
        fs::read_to_string(self.config_path()).unwrap()
    }
}

pub fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    let docs = r.join("docs");
    let notes = r.join("notes");
    fs::create_dir_all(docs.join("guide")).unwrap();
    fs::create_dir_all(docs.join("My Notes")).unwrap();
    fs::create_dir_all(&notes).unwrap();
    fs::write(docs.join("README.md"), "# Docs\n\n```mermaid\ngraph TD; A-->B\n```\n\n![i](img.png)\n").unwrap();
    fs::write(docs.join("img.png"), [0x89, b'P', b'N', b'G']).unwrap();
    fs::write(docs.join("guide/README.md"), "# Guide\n").unwrap();
    fs::write(docs.join("My Notes/é.md"), "# Accent\n").unwrap();
    fs::write(
        docs.join("page.html"),
        "<html><body><div class=\"mermaid\">graph TD;A-->B</div></body></html>",
    )
    .unwrap();
    fs::write(notes.join("n.md"), "# N\n").unwrap();
    let cfg = format!(
        "listen = \"127.0.0.1:0\"\nadmin_ip = \"{ADMIN}\"\nallowed_ips = [\"{ALLOWED}\"]\n\n\
         [[folders]]\nname = \"Docs\"\npath = {:?}\n\n[[folders]]\nname = \"Notes\"\npath = {:?}\n",
        docs.display().to_string(),
        notes.display().to_string()
    );
    let path = r.join("ziewer.toml");
    fs::write(&path, cfg).unwrap();
    let store = ConfigStore::load_with_env(&path, &Default::default()).unwrap();
    let state = build_state(store).unwrap();
    Fixture { dir, state }
}

async fn send(state: &AppState, mut req: Request<Body>, ip: &str) -> (StatusCode, HeaderMap, String) {
    let addr = SocketAddr::new(ip.parse().unwrap(), 5555);
    req.extensions_mut().insert(ConnectInfo(addr));
    let resp = router(state.clone()).oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&bytes).into_owned())
}

pub async fn get(state: &AppState, path: &str, ip: &str) -> (StatusCode, HeaderMap, String) {
    let req = Request::get(path).header("host", "localhost").body(Body::empty()).unwrap();
    send(state, req, ip).await
}

pub async fn post_form(
    state: &AppState,
    path: &str,
    ip: &str,
    body: &str,
    origin: Option<&str>,
) -> (StatusCode, HeaderMap, String) {
    let mut b = Request::post(path)
        .header("host", "localhost")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("user-agent", "test-agent");
    if let Some(o) = origin {
        b = b.header("origin", o);
    }
    send(state, b.body(Body::from(body.to_string())).unwrap(), ip).await
}

pub fn location(h: &HeaderMap) -> &str {
    h.get("location").unwrap().to_str().unwrap()
}
