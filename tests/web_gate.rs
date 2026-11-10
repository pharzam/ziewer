mod common;
use common::*;
use http::StatusCode;

#[tokio::test]
async fn denied_sees_ip_and_form() {
    let f = fixture();
    let (s, _, b) = get(&f.state, "/f/docs/", STRANGER).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(b.contains("8.8.8.8"));
    assert!(b.contains("Request access"));
    assert!(!b.contains("/f/notes/"), "folder menu leaked to denied user");
}

#[tokio::test]
async fn denied_on_root_and_unknown_paths() {
    let f = fixture();
    for p in ["/", "/nope", "/f/docs/README.md?raw=1", "/f/docs/img.png"] {
        let (s, _, _) = get(&f.state, p, STRANGER).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{p}");
    }
}

#[tokio::test]
async fn request_access_flow() {
    let f = fixture();
    let (s, _, b) = post_form(&f.state, "/request-access", STRANGER, "note=hi", None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b.contains("waiting for admin approval"), "{b}");
    let (s, _, b) = get(&f.state, "/", STRANGER).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(b.contains("pending"));
    assert!(f.state.requests.lock().unwrap().is_pending("8.8.8.8".parse().unwrap()));
}

#[tokio::test]
async fn second_request_rate_limited() {
    let f = fixture();
    post_form(&f.state, "/request-access", STRANGER, "note=a", None).await;
    let (s, _, _) = post_form(&f.state, "/request-access", STRANGER, "note=b", None).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn allowed_request_access_redirects() {
    let f = fixture();
    let (s, h, _) = post_form(&f.state, "/request-access", ALLOWED, "note=a", None).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert_eq!(location(&h), "/");
}

#[tokio::test]
async fn assets_open_to_denied() {
    let f = fixture();
    let (s, _, _) = get(&f.state, "/_assets/app.css", STRANGER).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn non_admin_blocked_from_admin() {
    let f = fixture();
    let (s, _, _) = get(&f.state, "/admin", ALLOWED).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = post_form(&f.state, "/admin/ips", ALLOWED, "ip=1.2.3.4", None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn bad_origin_rejected() {
    let f = fixture();
    let (s, _, _) = post_form(&f.state, "/request-access", STRANGER, "note=x", Some("http://evil.com")).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(f.state.requests.lock().unwrap().list().is_empty());
}

#[tokio::test]
async fn matching_origin_ok() {
    let f = fixture();
    let (s, _, _) = post_form(&f.state, "/request-access", STRANGER, "note=x", Some("http://localhost")).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn unknown_host_rejected_dns_rebinding() {
    let f = fixture();
    let (s, _, b) = get_with_host(&f.state, "/f/docs/", ADMIN, "evil.example:8080").await;
    assert_eq!(s, StatusCode::MISDIRECTED_REQUEST);
    assert!(!b.contains("Docs"));
}

#[tokio::test]
async fn ip_literal_and_localhost_hosts_accepted() {
    let f = fixture();
    for h in ["localhost:8080", "192.168.1.5:8080", "[::1]:8080", "LOCALHOST"] {
        let (s, _, _) = get_with_host(&f.state, "/f/docs/", ALLOWED, h).await;
        assert_eq!(s, StatusCode::OK, "{h}");
    }
}

#[tokio::test]
async fn configured_hostname_accepted() {
    let f = fixture_with_env(&[("ZIEWER_HOSTNAMES", "docs.example.com")]);
    let (s, _, _) = get_with_host(&f.state, "/f/docs/", ALLOWED, "Docs.Example.com:443").await;
    assert_eq!(s, StatusCode::OK);
}
