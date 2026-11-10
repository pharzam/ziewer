//! The 403 page for unknown IPs and the access-request endpoint.

use super::{AppState, ClientIp, base_ctx, error_page, render};
use crate::access::Role;
use crate::requests::SubmitError;
use axum::extract::{Form, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Extension;
use http::{HeaderMap, StatusCode, header};
use minijinja::context;
use serde::Deserialize;
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};

/// Render `denied.html` with the given status; `phase` is "form", "pending" or "sent".
pub fn denied_page(state: &AppState, ip: IpAddr, phase: &str, status: StatusCode) -> Response {
    let ctx = context! { title => "Access denied", ip => ip.to_string(), phase, ..base_ctx(state, Role::Denied, None) };
    (status, render(state, "denied.html", ctx)).into_response()
}

/// 403 for a denied client: shows "pending" if a request already exists, else the form.
pub fn denied(state: &AppState, ip: IpAddr) -> Response {
    let pending = state.requests.lock().unwrap().is_pending(ip);
    denied_page(state, ip, if pending { "pending" } else { "form" }, StatusCode::FORBIDDEN)
}

#[derive(Deserialize)]
pub struct RequestForm {
    #[serde(default)]
    note: String,
}

pub async fn request_access(
    State(state): State<AppState>,
    Extension(role): Extension<Role>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    headers: HeaderMap,
    Form(form): Form<RequestForm>,
) -> Response {
    if role != Role::Denied {
        return Redirect::to("/").into_response();
    }
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let result = state.requests.lock().unwrap().submit(ip, form.note.trim(), ua, now);
    match result {
        Ok(()) => {
            tracing::info!("access request from {ip}");
            denied_page(&state, ip, "sent", StatusCode::OK)
        }
        Err(SubmitError::RateLimited) => error_page(
            &state,
            role,
            StatusCode::TOO_MANY_REQUESTS,
            "Slow down",
            "You already sent a request recently. Please wait a few minutes.",
        ),
        Err(SubmitError::QueueFull) => error_page(
            &state,
            role,
            StatusCode::TOO_MANY_REQUESTS,
            "Too many requests",
            "The access request queue is full. Please try again later.",
        ),
        Err(SubmitError::Io(e)) => {
            tracing::error!("saving access request: {e}");
            error_page(&state, role, StatusCode::INTERNAL_SERVER_ERROR, "Error", "Could not save your request.")
        }
    }
}

/// True when an `Origin` header is present and its host[:port] differs from `Host`.
pub fn origin_mismatch(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return false };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let authority = origin.split_once("://").map(|(_, rest)| rest).unwrap_or("");
    let authority = authority.split('/').next().unwrap_or("");
    authority.is_empty() || !authority.eq_ignore_ascii_case(host)
}
