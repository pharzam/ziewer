# Ziewer — Design Spec

**Date:** 2026-11-10
**Status:** Draft — awaiting review

## 1. Purpose

Ziewer is a self-hosted, read-only web viewer that renders folders of Markdown and HTML
files with GitHub-style rendering (including Mermaid diagrams). Access is restricted by
client IP address; one admin IP manages who else may view and which folders are served.
It ships as a single Rust binary that can install itself as an OS service.

### Success criteria

- Configured root folders appear as links in a top menu; each is browsable recursively.
- `.md` files render like GitHub (GFM) with syntax highlighting and Mermaid diagrams.
- `.html` files are displayed, with Mermaid blocks rendered.
- A non-allowed IP sees a 403 page showing its IP and can request access.
- The admin IP can approve/deny requests and add/remove IPs and folders from a web page.
- `ziewer install` registers it as a service on Linux (systemd), macOS (launchd), Windows (SCM).

### Non-goals

Search, editing files, user accounts/passwords, live reload, HTTPS termination
(use a reverse proxy for TLS).

## 2. Architecture

Single binary `ziewer`, Rust (stable), crates:

| Concern | Crate |
|---|---|
| HTTP server | `axum`, `tokio`, `tower-http` |
| CLI | `clap` |
| Config | `serde`, `toml`, `dotenvy` |
| Markdown (GFM) | `comrak` (with syntect highlighting) |
| Templates | `minijinja` |
| Embedded assets | `rust-embed` |
| IP / CIDR | `ipnet` |
| OS service | `service-manager` |

Embedded assets (no CDN, works offline): `github-markdown-css` (light + dark),
`mermaid.min.js`, app CSS, templates.

### Modules

- `config` — load/validate/save `ziewer.toml`, apply env overrides. Shared as
  `Arc<RwLock<Config>>`; writes are atomic (write temp file, rename).
- `access` — resolve client IP, classify as `Admin | Allowed | Denied`.
- `requests` — pending access-request queue persisted to `requests.json`.
- `fs_tree` — safe path resolution inside a root; recursive tree of `.md`/`.html`.
- `render` — Markdown → HTML (comrak) with Mermaid post-processing.
- `web` — axum routes, handlers, templates.
- `service` — install/uninstall/start/stop via `service-manager`.

## 3. Configuration

File `ziewer.toml`. Path from `--config <path>`, else `ZIEWER_CONFIG`, else
`./ziewer.toml`. A `.env` file in the working directory is loaded first.

```toml
listen      = "0.0.0.0:8080"
admin_ip    = "192.168.1.10"
trust_proxy = false                 # true → client IP from X-Forwarded-For (rightmost entry)
allowed_ips = ["192.168.1.20", "10.0.0.0/24"]   # single IPs or CIDR

[[folders]]
name = "Docs"                       # menu label; URL slug derived (lowercase, dashes)
path = "/home/farzam/docs"
```

Env overrides (take precedence over file values at startup):
`ZIEWER_LISTEN`, `ZIEWER_ADMIN_IP`, `ZIEWER_TRUST_PROXY`,
`ZIEWER_ALLOWED_IPS` (comma-separated), `ZIEWER_FOLDERS` (`Name=/path,Name2=/path2`).
Values set via env are not written back when the admin saves; the admin page shows
them as "set by environment" and read-only.

Validation at startup (fail with clear message): `admin_ip` parses as an IP, every
`allowed_ips` entry parses as IP or CIDR, every folder path exists and is a directory,
folder slugs are unique.

The admin page persists changes to `ziewer.toml`. Pending requests live in
`requests.json` next to the config file.

## 4. Routes

| Route | Who | Behavior |
|---|---|---|
| `GET /` | allowed | Redirect to first folder (or "no folders configured" page) |
| `GET /f/{slug}/{path…}` | allowed | View directory or file (see below) |
| `GET /_assets/{file}` | everyone | Embedded CSS/JS |
| `POST /request-access` | denied | Queue access request (optional note ≤ 200 chars) |
| `GET /admin` | admin | Admin dashboard |
| `POST /admin/requests/{ip}/approve` | admin | Add IP to `allowed_ips`, remove request |
| `POST /admin/requests/{ip}/deny` | admin | Remove request |
| `POST /admin/ips` / `POST /admin/ips/delete` | admin | Add / remove allowed IP/CIDR |
| `POST /admin/folders` / `POST /admin/folders/delete` | admin | Add / remove folder |

### Viewing

- **Directory:** sidebar tree + render `README.md`, else `index.md`, else `index.html`,
  else a file listing.
- **`.md`:** rendered by comrak with GFM extensions (tables, task lists, strikethrough,
  autolinks, footnotes, heading IDs/anchors) and syntect highlighting, wrapped in
  `<article class="markdown-body">`. Raw HTML inside Markdown is allowed (trusted content).
- **`.html`:** body served inside an `<iframe>` with `src=/f/{slug}/{path}?raw=1`, so its own
  styles and scripts stay isolated; Ziewer injects `mermaid.min.js` + init script before
  `</body>` of the raw response so `class="mermaid"` elements render.
- **Other files** (images, PDFs, etc.): served raw with guessed content type, so relative
  links/images in Markdown work.
- **Hidden entries** (names starting with `.`) are excluded from trees and return 404.

### Layout

- **Top menu:** one link per configured folder (active one highlighted), plus "Admin" for
  the admin IP.
- **Left sidebar:** recursive collapsible tree of the current folder, listing only `.md`/
  `.html` files and directories that (recursively) contain them; current file highlighted.
- **Theme:** follows OS `prefers-color-scheme`; Mermaid theme matches.

### Mermaid

- Markdown: fenced ` ```mermaid ` blocks are emitted as `<pre class="mermaid">…</pre>`
  (escaped source) instead of highlighted code.
- Page includes embedded `mermaid.min.js` and calls `mermaid.initialize({startOnLoad:true,
  theme: dark ? 'dark' : 'default'})`.

## 5. Access Control

Every request (except `/_assets/*`) passes through middleware:

1. Determine client IP: socket peer address; if `trust_proxy`, the rightmost
   `X-Forwarded-For` entry (the one the trusted proxy appended) instead.
2. IPv4-mapped IPv6 addresses are normalized to IPv4.
3. Classify: equals `admin_ip` → **Admin**; matches any `allowed_ips` entry → **Allowed**;
   else **Denied**.
4. `/admin*` requires Admin, otherwise 403.
5. Denied clients get the **403 page** for every route except `POST /request-access`.

### 403 page

Shows "Access denied", **"Your IP: x.x.x.x"**, and a form with an optional note and a
**Request access** button. After submitting: "Request sent, waiting for admin approval."
If a request for that IP is already pending, it shows that state instead of the form.

### Request queue

- Entry: `{ ip, note, user_agent, requested_at }`, keyed by IP (one pending per IP).
- Limits: max 100 pending requests total; a given IP may re-submit at most once per
  10 minutes. Excess → 429.

### Admin dashboard

Three sections: **Pending requests** (IP, note, user agent, time, Approve/Deny),
**Allowed IPs** (list with Remove, add form accepting IP or CIDR), **Folders** (name,
path, Remove, add form with name + absolute path — validated as existing directory).
Admin IP itself is shown read-only (change it in the config file).

### Hardening

- All mutations are POST; requests whose `Origin` header (if present) does not match the
  `Host` are rejected (CSRF protection without tokens).
- Path safety: requested path joined to root, `canonicalize`d, and must `starts_with` the
  canonical root — blocks `..` and symlinks pointing outside. Failure → 404.
- Security headers: `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin`,
  `X-Frame-Options: SAMEORIGIN`.

## 6. OS Service

```
ziewer serve   [--config PATH]   # run in foreground
ziewer install [--config PATH]   # register service (label "ziewer"), absolute config path baked in
ziewer uninstall
ziewer start | stop
```

- Linux: systemd system unit (requires root) — `Restart=on-failure`.
- macOS: launchd daemon. Windows: Service Control Manager.
- `install` refuses if the config file doesn't exist or fails validation.
- Logs go to stdout/stderr (journald on Linux) via `tracing`.

## 7. Error Handling

| Situation | Result |
|---|---|
| Unknown folder slug / missing file / path escapes root / hidden file | 404 page |
| Non-allowed IP | 403 page with IP + request form |
| Non-admin on `/admin*` | 403 |
| Invalid admin form input (bad IP, missing dir) | Admin page re-rendered with error message |
| Request flood | 429 |
| Invalid config at startup | Exit non-zero with message naming the bad field |
| Config write failure | 500 on admin page with message; in-memory state unchanged |

## 8. Testing

- **Unit:** IP/CIDR matching and IPv4-mapped normalization; `X-Forwarded-For` handling;
  path containment (`..`, symlink escape, hidden files); Markdown→HTML Mermaid conversion;
  config load/env override/save round-trip; request queue limits.
- **Integration** (axum `oneshot` with fake peer addr): allowed IP views file; denied IP
  gets 403 with its IP shown; request-access → appears on admin page → approve → IP can
  view; non-admin blocked from `/admin`; Origin mismatch rejected.
