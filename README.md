# Ziewer

Ziewer is a small, self-hosted, read-only viewer for folders of Markdown and HTML files. It renders them the way GitHub does, including Mermaid diagrams. Access is limited by IP address: one admin IP decides who else can view and which folders are served.

- **Top menu:** one link per configured root folder.
- **Files drawer:** the **☰ Files** button opens the recursive tree of `.md` and `.html` files in the current folder; pages use the full width.
- **Markdown:** GitHub-flavoured (tables, task lists, footnotes, heading anchors, syntax highlighting) with ` ```mermaid ` diagrams.
- **HTML pages:** open as themselves, full page with their own styles and scripts. Elements with `class="mermaid"` are rendered as diagrams. A folder whose index is `index.html` opens that page.
- **Unknown IPs:** they see a 403 page with their IP and a **Request access** button. The admin approves or denies requests at `/admin`.
- **Self-contained:** a single binary with all assets built in, so no CDN is needed. It can install itself as an OS service.

## Build

```sh
cargo build --release
sudo cp target/release/ziewer /usr/local/bin/
```

## Configure

Copy [`ziewer.example.toml`](ziewer.example.toml) to `ziewer.toml` and edit it:

```toml
listen      = "0.0.0.0:8080"
admin_ip    = "192.168.1.10"
trust_proxy = false
allowed_ips = ["192.168.1.20", "10.0.0.0/24"]   # single IPs or CIDR
hostnames   = ["docs.example.com"]               # optional: names besides IPs/localhost

[[folders]]
name = "Docs"                 # menu label; URL becomes /f/docs/
path = "/home/me/docs"        # absolute path
```

Ziewer looks for the config file in this order: `--config <path>`, then `ZIEWER_CONFIG`, then `./ziewer.toml`. A `.env` file in the working directory is loaded first.

### Environment overrides

| Variable | Example |
|---|---|
| `ZIEWER_LISTEN` | `0.0.0.0:8080` |
| `ZIEWER_ADMIN_IP` | `192.168.1.10` |
| `ZIEWER_TRUST_PROXY` | `true` |
| `ZIEWER_ALLOWED_IPS` | `192.168.1.20,10.0.0.0/24` |
| `ZIEWER_FOLDERS` | `Docs=/home/me/docs,Notes=/srv/notes` |
| `ZIEWER_HOSTNAMES` | `docs.example.com` |

Values set through the environment take precedence over the file. They are shown read-only on the admin page and are never written back to the file.

## Run

```sh
ziewer serve --config ./ziewer.toml
```

Open `http://<host>:8080/`. From the admin IP, `/admin` lets you:

- approve or deny pending access requests (an approved IP is added to `allowed_ips`)
- add or remove allowed IPs and CIDR ranges
- add or remove folders

Changes are saved back to `ziewer.toml`. Pending requests are stored in `requests.json` next to it.

## Install as a service

Ziewer can register itself with systemd (Linux), launchd (macOS) or the Windows Service Manager.

```sh
sudo mkdir -p /etc/ziewer && sudo cp ziewer.toml /etc/ziewer/
sudo ziewer install --config /etc/ziewer/ziewer.toml    # installs and starts
sudo ziewer stop | start
sudo ziewer uninstall
```

### Updating the installed service

After changing the code, rebuild and redeploy in one step:

```sh
scripts/deploy.sh               # test, build, stop, install, start, show status
SKIP_TESTS=1 scripts/deploy.sh  # skip the test run
```

On Linux, logs go to the journal: `journalctl -u ziewer -f`. The config path and the binary path must not contain spaces.

## Host names

Ziewer only answers requests whose `Host` is an IP address, `localhost`, or a name listed in `hostnames`. Anything else gets a 421 response. This blocks DNS-rebinding attacks, where a malicious website re-points its own domain at your LAN and uses the admin's browser, which sits on a trusted IP. If you reach Ziewer by a name such as `docs.lan`, add that name to `hostnames`.

## Behind a reverse proxy

Ziewer serves plain HTTP. For HTTPS, put it behind nginx or Caddy and set `trust_proxy = true`. Ziewer then uses the **rightmost** `X-Forwarded-For` entry, which is the one your proxy appended. Entries to its left come from the client and are ignored.

- Only enable `trust_proxy` when the proxy is the only way to reach Ziewer, for example when Ziewer listens on `127.0.0.1`. Otherwise clients can send the header themselves.
- The proxy must pass the original `Host` header, and that name must be in `hostnames`.

```nginx
location / {
    proxy_pass http://127.0.0.1:8080;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
}
```

## Security notes

- Requested paths are resolved inside their root folder. `..`, hidden files (names starting with `.`) and symlinks pointing outside the root all return 404.
- Every change is a POST, and cross-site form posts are rejected by checking `Origin` against `Host`.
- Access requests are limited to one per IP every 10 minutes and 100 pending in total.
- Folder contents are treated as trusted: raw HTML inside Markdown and scripts inside HTML pages run in viewers' browsers.
