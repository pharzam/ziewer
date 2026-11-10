mod common;
use common::*;
use http::StatusCode;

#[tokio::test]
async fn root_redirects_to_first_folder() {
    let f = fixture();
    let (s, h, _) = get(&f.state, "/", ALLOWED).await;
    assert_eq!(s, StatusCode::FOUND);
    assert_eq!(location(&h), "/f/docs/");
}

#[tokio::test]
async fn top_menu_lists_folders() {
    let f = fixture();
    let (s, _, b) = get(&f.state, "/f/docs/", ALLOWED).await;
    assert_eq!(s, StatusCode::OK);
    for needle in ["/f/docs/", "/f/notes/", "Docs", "Notes"] {
        assert!(b.contains(needle), "missing {needle}");
    }
    assert!(!b.contains("/admin"), "admin link shown to non-admin");
}

#[tokio::test]
async fn admin_sees_admin_link() {
    let f = fixture();
    let (_, _, b) = get(&f.state, "/f/docs/", ADMIN).await;
    assert!(b.contains("href=\"/admin\""));
}

#[tokio::test]
async fn folder_renders_readme_with_mermaid() {
    let f = fixture();
    let (_, _, b) = get(&f.state, "/f/docs/", ALLOWED).await;
    assert!(b.contains("markdown-body"));
    assert!(b.contains("<pre class=\"mermaid\">"));
    assert!(b.contains("mermaid.min.js"));
}

#[tokio::test]
async fn dir_without_slash_redirects() {
    let f = fixture();
    let (s, h, _) = get(&f.state, "/f/docs/guide", ALLOWED).await;
    assert_eq!(s, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(location(&h), "/f/docs/guide/");
    let (s, h, _) = get(&f.state, "/f/docs", ALLOWED).await;
    assert_eq!(s, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(location(&h), "/f/docs/");
}

#[tokio::test]
async fn unicode_and_space_path() {
    let f = fixture();
    let (_, _, b) = get(&f.state, "/f/docs/", ALLOWED).await;
    assert!(b.contains("/f/docs/My%20Notes/%C3%A9.md"), "{b}");
    let (s, _, b) = get(&f.state, "/f/docs/My%20Notes/%C3%A9.md", ALLOWED).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b.contains("Accent"));
}

#[tokio::test]
async fn html_opens_as_standalone_page() {
    let f = fixture();
    let (s, h, b) = get(&f.state, "/f/docs/page.html", ALLOWED).await;
    assert_eq!(s, StatusCode::OK);
    assert!(h["content-type"].to_str().unwrap().starts_with("text/html"));
    assert!(b.starts_with("<html><body><div class=\"mermaid\">"), "not the file itself: {b}");
    assert!(!b.contains("topbar") && !b.contains("html-frame"), "wrapped in the viewer");
    assert!(b.find("mermaid.min.js").unwrap() < b.find("</body>").unwrap());
}

#[tokio::test]
async fn html_links_in_tree_point_to_file() {
    let f = fixture();
    let (_, _, b) = get(&f.state, "/f/docs/", ALLOWED).await;
    assert!(b.contains("href=\"/f/docs/page.html\""));
}

#[tokio::test]
async fn directory_with_only_index_html_serves_it() {
    let f = fixture();
    let site = f.dir.path().join("docs/site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.html"), "<html><body>SITE</body></html>").unwrap();
    let (s, _, b) = get(&f.state, "/f/docs/site/", ALLOWED).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b.starts_with("<html><body>SITE"), "{b}");
}

#[tokio::test]
async fn markdown_raw_is_text() {
    let f = fixture();
    let (s, h, b) = get(&f.state, "/f/docs/README.md?raw=1", ALLOWED).await;
    assert_eq!(s, StatusCode::OK);
    assert!(h["content-type"].to_str().unwrap().starts_with("text/plain"));
    assert!(b.starts_with("# Docs"));
}

#[tokio::test]
async fn image_served_raw() {
    let f = fixture();
    let (s, h, _) = get(&f.state, "/f/docs/img.png", ALLOWED).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h["content-type"], "image/png");
}

#[tokio::test]
async fn traversal_is_404() {
    let f = fixture();
    let (s, _, _) = get(&f.state, "/f/docs/..%2F..%2Fetc%2Fpasswd", ALLOWED).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = get(&f.state, "/f/nope/", ALLOWED).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = get(&f.state, "/f/docs/missing.md", ALLOWED).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn assets_served() {
    let f = fixture();
    let (s, h, _) = get(&f.state, "/_assets/vendor/mermaid.min.js", ALLOWED).await;
    assert_eq!(s, StatusCode::OK);
    assert!(h["content-type"].to_str().unwrap().contains("javascript"));
    let (s, _, _) = get(&f.state, "/_assets/nope.js", ALLOWED).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn security_headers_present() {
    let f = fixture();
    for p in ["/f/docs/", "/f/nope/", "/_assets/app.css"] {
        let (_, h, _) = get(&f.state, p, ALLOWED).await;
        assert_eq!(h["x-content-type-options"], "nosniff", "{p}");
        assert_eq!(h["referrer-policy"], "same-origin", "{p}");
        assert_eq!(h["x-frame-options"], "SAMEORIGIN", "{p}");
    }
}

#[tokio::test]
async fn full_width_layout_with_file_drawer() {
    let f = fixture();
    let (_, _, b) = get(&f.state, "/f/docs/README.md", ALLOWED).await;
    assert!(!b.contains("with-sidebar"), "sidebar layout still used");
    assert!(b.contains("id=\"files-toggle\""), "no Files button");
    assert!(b.contains("class=\"drawer\""), "no file drawer");
    assert!(b.contains("/f/docs/My%20Notes/%C3%A9.md"), "drawer lost the tree");
}
