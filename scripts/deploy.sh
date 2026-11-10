#!/usr/bin/env bash
# Build a release, then stop the ziewer service, install the new binary and start it again.
# Usage: scripts/deploy.sh            (run from anywhere in the repo; asks for sudo once)
#        SKIP_TESTS=1 scripts/deploy.sh
set -euo pipefail

BIN_DEST=/usr/local/bin/ziewer
SERVICE=ziewer

cd "$(dirname "$0")/.."

if [[ "${SKIP_TESTS:-0}" != 1 ]]; then
  echo "==> Running tests"
  cargo test --quiet
fi

echo "==> Building release"
cargo build --release

echo "==> Stopping $SERVICE (sudo)"
sudo systemctl stop "$SERVICE"

echo "==> Installing binary to $BIN_DEST"
sudo install -m 755 target/release/ziewer "$BIN_DEST"

echo "==> Starting $SERVICE"
sudo systemctl start "$SERVICE"

sleep 1
if systemctl is-active --quiet "$SERVICE"; then
  echo "==> $SERVICE is running: $("$BIN_DEST" --version)"
  journalctl -u "$SERVICE" -n 3 --no-pager
else
  echo "!! $SERVICE failed to start" >&2
  journalctl -u "$SERVICE" -n 20 --no-pager >&2
  exit 1
fi
