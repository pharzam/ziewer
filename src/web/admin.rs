//! Admin dashboard: access requests, allowed IPs and folders.

use super::{AppState, base_ctx, render};
use crate::access::Role;
use crate::config::{Config, ConfigError, Folder, parse_ip_rule, slugify};
use axum::extract::{Form, Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use http::StatusCode;
use minijinja::context;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::PathBuf;

const LOCKED: &str = "Set by environment: change it in the environment / .env, not here.";

#[derive(Serialize)]
struct RequestRow {
    ip: String,
    note: String,
    user_agent: String,
    requested_at: String,
}

#[derive(Serialize)]
struct FolderRow {
    name: String,
    path: String,
    slug: String,
}

/// Format unix seconds as `YYYY-MM-DD HH:MM UTC`.
fn fmt_time(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem % 3600) / 60)
}

fn page(state: &AppState, error: Option<&str>, status: StatusCode) -> Response {
    let (cfg, locks): (Config, _) = {
        let store = state.config.read().unwrap();
        (store.effective().clone(), store.locks())
    };
    let requests: Vec<RequestRow> = state
        .requests
        .lock()
        .unwrap()
        .list()
        .into_iter()
        .map(|r| RequestRow {
            ip: r.ip.to_string(),
            note: r.note,
            user_agent: r.user_agent,
            requested_at: fmt_time(r.requested_at),
        })
        .collect();
    let folders: Vec<FolderRow> = cfg
        .folders
        .iter()
        .map(|f| FolderRow { name: f.name.clone(), path: f.path.display().to_string(), slug: slugify(&f.name) })
        .collect();
    let ctx = context! {
        title => "Admin",
        admin_ip => cfg.admin_ip,
        allowed_ips => cfg.allowed_ips,
        requests,
        admin_folders => folders,
        locks,
        error,
        ..base_ctx(state, Role::Admin, None)
    };
    (status, render(state, "admin.html", ctx)).into_response()
}

fn bad(state: &AppState, msg: &str) -> Response {
    page(state, Some(msg), StatusCode::BAD_REQUEST)
}

fn done() -> Response {
    Redirect::to("/admin").into_response()
}

/// Apply a config change, mapping errors to a 400 admin page.
#[allow(clippy::result_large_err)] // Err is a ready-to-return HTTP response
fn apply(state: &AppState, f: impl Fn(&mut Config)) -> Result<(), Response> {
    let result = state.config.write().unwrap().update(f);
    result.map_err(|e| match e {
        ConfigError::Invalid(m) => bad(state, &m),
        other => {
            tracing::error!("saving config: {other}");
            page(state, Some(&other.to_string()), StatusCode::INTERNAL_SERVER_ERROR)
        }
    })
}

fn locks(state: &AppState) -> crate::config::EnvLocks {
    state.config.read().unwrap().locks()
}

pub async fn dashboard(State(state): State<AppState>) -> Response {
    page(&state, None, StatusCode::OK)
}

pub async fn approve(State(state): State<AppState>, Path(ip): Path<String>) -> Response {
    let Ok(ip) = ip.parse::<IpAddr>() else { return bad(&state, "Invalid IP or CIDR") };
    if locks(&state).allowed_ips {
        return bad(&state, LOCKED);
    }
    let rule = ip.to_string();
    if let Err(r) = apply(&state, |c| {
        if !c.allowed_ips.contains(&rule) {
            c.allowed_ips.push(rule.clone());
        }
    }) {
        return r;
    }
    if let Err(e) = state.requests.lock().unwrap().remove(ip) {
        tracing::error!("removing request: {e:?}");
    }
    tracing::info!("admin approved {ip}");
    done()
}

pub async fn deny(State(state): State<AppState>, Path(ip): Path<String>) -> Response {
    let Ok(ip) = ip.parse::<IpAddr>() else { return bad(&state, "Invalid IP or CIDR") };
    if let Err(e) = state.requests.lock().unwrap().remove(ip) {
        tracing::error!("removing request: {e:?}");
    }
    done()
}

#[derive(Deserialize)]
pub struct IpForm {
    ip: String,
}

pub async fn add_ip(State(state): State<AppState>, Form(form): Form<IpForm>) -> Response {
    if locks(&state).allowed_ips {
        return bad(&state, LOCKED);
    }
    let rule = form.ip.trim().to_string();
    if parse_ip_rule(&rule).is_none() {
        return bad(&state, "Invalid IP or CIDR");
    }
    if state.config.read().unwrap().effective().allowed_ips.contains(&rule) {
        return done();
    }
    match apply(&state, |c| c.allowed_ips.push(rule.clone())) {
        Ok(()) => done(),
        Err(r) => r,
    }
}

pub async fn delete_ip(State(state): State<AppState>, Form(form): Form<IpForm>) -> Response {
    if locks(&state).allowed_ips {
        return bad(&state, LOCKED);
    }
    let rule = form.ip.trim().to_string();
    match apply(&state, |c| c.allowed_ips.retain(|r| r != &rule)) {
        Ok(()) => done(),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
pub struct FolderForm {
    name: String,
    #[serde(default)]
    path: String,
}

pub async fn add_folder(State(state): State<AppState>, Form(form): Form<FolderForm>) -> Response {
    if locks(&state).folders {
        return bad(&state, LOCKED);
    }
    let folder = Folder { name: form.name.trim().to_string(), path: PathBuf::from(form.path.trim()) };
    if folder.name.is_empty() || form.path.trim().is_empty() {
        return bad(&state, "Folder name and path are required.");
    }
    match apply(&state, |c| c.folders.push(folder.clone())) {
        Ok(()) => done(),
        Err(r) => r,
    }
}

pub async fn delete_folder(State(state): State<AppState>, Form(form): Form<FolderForm>) -> Response {
    if locks(&state).folders {
        return bad(&state, LOCKED);
    }
    let name = form.name.trim().to_string();
    match apply(&state, |c| c.folders.retain(|f| f.name != name)) {
        Ok(()) => done(),
        Err(r) => r,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fmt_time_known_values() {
        assert_eq!(super::fmt_time(0), "1970-01-01 00:00 UTC");
        assert_eq!(super::fmt_time(1_794_312_000), "2026-11-10 12:00 UTC");
    }
}
