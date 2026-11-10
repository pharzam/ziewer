mod common;
use common::*;
use http::StatusCode;

async fn request(f: &Fixture, note: &str) {
    let body = format!("note={}", urlencode(note));
    let (s, _, _) = post_form(&f.state, "/request-access", STRANGER, &body, None).await;
    assert_eq!(s, StatusCode::OK);
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tokio::test]
async fn admin_page_renders() {
    let f = fixture();
    let (s, _, b) = get(&f.state, "/admin", ADMIN).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b.contains("10.0.0.1") && b.contains("10.0.0.2") && b.contains("Docs"));
}

#[tokio::test]
async fn approve_flow() {
    let f = fixture();
    request(&f, "please").await;
    let (_, _, b) = get(&f.state, "/admin", ADMIN).await;
    assert!(b.contains("8.8.8.8") && b.contains("please") && b.contains("test-agent"));
    let (s, h, _) = post_form(&f.state, "/admin/requests/8.8.8.8/approve", ADMIN, "", None).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert_eq!(location(&h), "/admin");
    let (s, _, _) = get(&f.state, "/f/docs/", STRANGER).await;
    assert_eq!(s, StatusCode::OK);
    assert!(f.config_text().contains("8.8.8.8"));
    assert!(!f.state.requests.lock().unwrap().is_pending("8.8.8.8".parse().unwrap()));
}

#[tokio::test]
async fn deny_flow() {
    let f = fixture();
    request(&f, "x").await;
    let (s, _, _) = post_form(&f.state, "/admin/requests/8.8.8.8/deny", ADMIN, "", None).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert!(f.state.requests.lock().unwrap().list().is_empty());
    let (s, _, _) = get(&f.state, "/f/docs/", STRANGER).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn note_is_escaped() {
    let f = fixture();
    request(&f, "<script>alert(1)</script>").await;
    let (_, _, b) = get(&f.state, "/admin", ADMIN).await;
    assert!(b.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!b.contains("<script>alert(1)"));
}

#[tokio::test]
async fn add_remove_ip() {
    let f = fixture();
    let (s, _, _) = post_form(&f.state, "/admin/ips", ADMIN, "ip=172.16.0.0%2F16", None).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    let (s, _, _) = get(&f.state, "/f/docs/", "172.16.5.5").await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = post_form(&f.state, "/admin/ips/delete", ADMIN, "ip=172.16.0.0%2F16", None).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    let (s, _, _) = get(&f.state, "/f/docs/", "172.16.5.5").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn duplicate_ip_noop() {
    let f = fixture();
    let (s, _, _) = post_form(&f.state, "/admin/ips", ADMIN, "ip=10.0.0.2", None).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert_eq!(f.config_text().matches("10.0.0.2").count(), 1);
}

#[tokio::test]
async fn invalid_ip_400() {
    let f = fixture();
    let before = f.config_text();
    let (s, _, b) = post_form(&f.state, "/admin/ips", ADMIN, "ip=nope", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b.contains("Invalid IP or CIDR"));
    assert_eq!(f.config_text(), before);
}

#[tokio::test]
async fn add_folder() {
    let f = fixture();
    let extra = f.dir.path().join("extra");
    std::fs::create_dir_all(&extra).unwrap();
    std::fs::write(extra.join("e.md"), "# E").unwrap();
    let body = format!("name=Extra&path={}", urlencode(&extra.display().to_string()));
    let (s, _, _) = post_form(&f.state, "/admin/folders", ADMIN, &body, None).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    let (_, _, b) = get(&f.state, "/f/docs/", ALLOWED).await;
    assert!(b.contains("/f/extra/"));
    let (s, _, _) = get(&f.state, "/f/extra/e.md", ALLOWED).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = post_form(&f.state, "/admin/folders/delete", ADMIN, "name=Extra", None).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    let (s, _, _) = get(&f.state, "/f/extra/e.md", ALLOWED).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn folder_slug_collision() {
    let f = fixture();
    let before = f.config_text();
    let body = format!("name=DOCS&path={}", urlencode(&f.dir.path().join("notes").display().to_string()));
    let (s, _, b) = post_form(&f.state, "/admin/folders", ADMIN, &body, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b.contains("slug"));
    assert_eq!(f.config_text(), before);
}

#[tokio::test]
async fn missing_folder_path() {
    let f = fixture();
    let (s, _, _) = post_form(&f.state, "/admin/folders", ADMIN, "name=X&path=%2Fdoes%2Fnot%2Fexist", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn env_locked_ips_rejected() {
    let f = fixture_with_env(&[("ZIEWER_ALLOWED_IPS", "10.0.0.2")]);
    let before = f.config_text();
    let (s, _, b) = post_form(&f.state, "/admin/ips", ADMIN, "ip=1.2.3.4", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b.contains("Set by environment"));
    assert_eq!(f.config_text(), before);
    let (_, _, b) = get(&f.state, "/admin", ADMIN).await;
    assert!(b.contains("disabled"));
}
