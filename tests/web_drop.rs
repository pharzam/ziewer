mod common;
use common::*;
use http::StatusCode;

const MD: &str = "# Dropped\n\nسلام دنیا\n\n```mermaid\ngraph TD; A-->B\n```\n";

#[tokio::test]
async fn render_endpoint_renders_markdown() {
    let f = fixture();
    let (s, h, b) = post_body(&f.state, "/render", ALLOWED, "text/markdown; charset=utf-8", MD.into()).await;
    assert_eq!(s, StatusCode::OK);
    assert!(h["content-type"].to_str().unwrap().starts_with("text/html"));
    assert!(b.contains("<h1") && b.contains("Dropped"), "{b}");
    assert!(b.contains("<p dir=\"auto\">سلام دنیا</p>"), "{b}");
    assert!(b.contains("<pre class=\"mermaid\">"), "{b}");
    assert!(!b.contains("<html"), "should be a fragment, not a page");
}

#[tokio::test]
async fn render_endpoint_saves_nothing() {
    let f = fixture();
    let before: Vec<_> = walk(f.dir.path());
    post_body(&f.state, "/render", ADMIN, "text/markdown", MD.into()).await;
    assert_eq!(walk(f.dir.path()), before);
}

#[tokio::test]
async fn render_endpoint_denied_for_strangers() {
    let f = fixture();
    let (s, _, b) = post_body(&f.state, "/render", STRANGER, "text/markdown", MD.into()).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(!b.contains("Dropped"));
}

#[tokio::test]
async fn render_endpoint_rejects_large_files() {
    let f = fixture();
    let big = vec![b'a'; 5 * 1024 * 1024 + 1];
    let (s, _, _) = post_body(&f.state, "/render", ALLOWED, "text/markdown", big).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn render_endpoint_rejects_non_utf8() {
    let f = fixture();
    let (s, _, _) = post_body(&f.state, "/render", ALLOWED, "text/markdown", vec![0xff, 0xfe, 0x00]).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn viewer_pages_enable_drop() {
    let f = fixture();
    let (_, _, b) = get(&f.state, "/f/docs/", ALLOWED).await;
    assert!(b.contains("data-drop=\"1\""), "drop not enabled for allowed user");
    let (_, _, b) = get(&f.state, "/f/docs/", STRANGER).await;
    assert!(!b.contains("data-drop=\"1\""), "drop enabled on 403 page");
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}
