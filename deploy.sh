#!/usr/bin/env bash
# deploy.sh — upload and hot-deploy the latest Quorum build to the server
#
# Usage:
#   ./deploy.sh                         # deploy to default server (34.59.112.246)
#   ./deploy.sh user@1.2.3.4            # deploy to a different server
#   ./deploy.sh --zip-only              # just package the zip, don't upload
#
# What it does:
#   1. Build frontend (npm run build)
#   2. Package quorum-linux-runtime.zip
#   3. Upload the zip to the server
#   4. On the server: stop backend + AI, replace files, start everything back
#   5. Run a health check

set -euo pipefail

# ─── config ───────────────────────────────────────────────────────────────────
SERVER="${1:-chavis_mtech@34.59.112.246}"
REMOTE_DIR="/home/chavis_mtech/quorum-linux-runtime"   # where quorum is installed on the server
ZIP_ONLY=false

# Optional dedicated SSH key (e.g. a passphrase-less deploy key). When SSH_KEY is set, scp/ssh use
# ONLY this key (IdentitiesOnly) — lets an automated agent deploy without touching the user's other keys.
#   usage: SSH_KEY=~/.ssh/quorum_deploy ./deploy.sh
SSH_KEY="${SSH_KEY:-}"
SSH_OPTS=()
[ -n "$SSH_KEY" ] && SSH_OPTS=(-i "$SSH_KEY" -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new)

if [ "${1:-}" = "--zip-only" ]; then
  ZIP_ONLY=true
  SERVER=""
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

BUILD_DATE=$(date '+%Y%m%d-%H%M%S')
ZIP_NAME="quorum-linux-runtime-${BUILD_DATE}.zip"
RUNTIME_DIR="$SCRIPT_DIR/quorum-linux-runtime"
BINARY="$SCRIPT_DIR/backend/target/x86_64-unknown-linux-gnu/release/quorum"

# ─── colours ──────────────────────────────────────────────────────────────────
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
step() { echo -e "\n${GREEN}[+]${NC} $*"; }
warn() { echo -e "${YELLOW}[!]${NC} $*"; }
die()  { echo -e "${RED}[✗]${NC} $*" >&2; exit 1; }

# ─── 1. check binary exists ───────────────────────────────────────────────────
step "Checking Linux binary..."
if [ ! -f "$BINARY" ]; then
  die "Linux binary not found at $BINARY

  Build it first:
    brew install zig                              # one-time
    cargo install cargo-zigbuild                  # one-time
    rustup target add x86_64-unknown-linux-gnu    # one-time
    cd backend && cargo zigbuild --target x86_64-unknown-linux-gnu --release"
fi

BINARY_SHA=$(shasum -a 256 "$BINARY" | awk '{print $1}')
echo "  binary: $(ls -lh "$BINARY" | awk '{print $5}')  sha256: ${BINARY_SHA:0:16}…"

# ─── 2. build frontend ────────────────────────────────────────────────────────
step "Building frontend..."
cd frontend && npm run build --silent && cd ..
echo "  $(ls -lh frontend/dist/assets/*.js | awk '{print $NF, $5}')"

# ─── 3. assemble runtime dir ──────────────────────────────────────────────────
step "Assembling runtime package..."

cp "$BINARY" "$RUNTIME_DIR/backend/quorum"
chmod +x "$RUNTIME_DIR/backend/quorum"

rm -rf "$RUNTIME_DIR/frontend/dist"
mkdir -p "$RUNTIME_DIR/frontend/dist"
cp -r frontend/dist/* "$RUNTIME_DIR/frontend/dist/"

rm -rf "$RUNTIME_DIR/ai-layer"
cp -r ai-layer "$RUNTIME_DIR/ai-layer"

mkdir -p "$RUNTIME_DIR/db/migrations"
cp db/migrations/*.sql "$RUNTIME_DIR/db/migrations/"

# ─── 4. write build info ──────────────────────────────────────────────────────
cat > "$RUNTIME_DIR/BUILD_INFO.txt" << BUILDEOF
Quorum Linux runtime package
Repacked at: $(date '+%Y-%m-%d %H:%M:%S %z') (broker-coin fix + regime-aware entries)

This build adds:
- Bitkub "broker coin" fix (root cause of failed live buys / error 61): the discovery
  universe now keeps only source="exchange", active, buy-unfrozen coins, and the broker
  rejects a broker-coin order up front with a clear message instead of firing a doomed
  request. Error 61 is now mapped to a human-readable reason.
- Regime-aware entry style (Python judge): a clean trend now enters AT MARKET to ride the
  move (no more waiting for a pullback that never comes), while ranging markets still buy
  dips to support and only truly parabolic moves are converted to a pullback LIMIT.
- Robust live cash read: a transient 0-balance read is retried before blocking, and the
  old misleading "risk cap reduced order size to 0" is now an accurate cash message.
- Trades view now shows the broker failure reason (note) inline on failed orders.

Carried over: deterministic realized P&L, always-on hard stop + catastrophic loss cap,
fixed-% profit lock, live/paper dashboard badge, active position management (trailing).

Migrations run automatically on boot.

Cross-built on macOS/arm64 via cargo-zigbuild + zig (linux/amd64 glibc).
Tested: 66 cargo tests pass, python aggregator (5) + entry-discipline (13) tests pass, tsc clean, vite build clean.

binary sha256: ${BINARY_SHA}
BUILDEOF

# ─── 5. pack zip ──────────────────────────────────────────────────────────────
step "Packing zip..."
rm -f "$SCRIPT_DIR/$ZIP_NAME"
cd "$SCRIPT_DIR"
zip -r "$ZIP_NAME" quorum-linux-runtime/ -x "*.DS_Store" -x "__pycache__/*" -x "*.pyc" -q
ls -lh "$ZIP_NAME"

if $ZIP_ONLY; then
  echo -e "\n${GREEN}Done!${NC} Zip ready: $ZIP_NAME"
  echo "Upload manually: scp $ZIP_NAME ${SERVER:-user@SERVER}:~"
  exit 0
fi

# ─── 6. upload ────────────────────────────────────────────────────────────────
step "Uploading to ${SERVER}..."
scp "${SSH_OPTS[@]}" "$ZIP_NAME" "${SERVER}:~/"
echo "  uploaded"

# ─── 7. remote deploy ─────────────────────────────────────────────────────────
step "Deploying on server..."
ssh "${SSH_OPTS[@]}" "$SERVER" bash << REMOTE
set -euo pipefail

ZIP="$ZIP_NAME"
QUORUM_DIR="$REMOTE_DIR"

echo "[server] stopping backend and AI sidecar..."
if [ -d "\$QUORUM_DIR" ]; then
  cd "\$QUORUM_DIR"
  [ -f scripts/stop.sh ] && bash scripts/stop.sh || true
fi

echo "[server] extracting zip..."
cd ~
unzip -o "\$ZIP" -d quorum_update_tmp > /dev/null
UPDATE_DIR="quorum_update_tmp/quorum-linux-runtime"

if [ ! -d "\$QUORUM_DIR" ]; then
  echo "[server] fresh install..."
  mv "\$UPDATE_DIR" "\$QUORUM_DIR"
else
  echo "[server] hot-update binary + frontend + ai-layer + migrations..."
  cp "\$UPDATE_DIR/backend/quorum" "\$QUORUM_DIR/backend/quorum"
  chmod +x "\$QUORUM_DIR/backend/quorum"
  rm -rf "\$QUORUM_DIR/frontend/dist"
  cp -r "\$UPDATE_DIR/frontend/dist" "\$QUORUM_DIR/frontend/dist"
  rm -rf "\$QUORUM_DIR/ai-layer"
  cp -r "\$UPDATE_DIR/ai-layer" "\$QUORUM_DIR/ai-layer"
  cp "\$UPDATE_DIR/db/migrations/"*.sql "\$QUORUM_DIR/db/migrations/"
  cp "\$UPDATE_DIR/BUILD_INFO.txt" "\$QUORUM_DIR/BUILD_INFO.txt"
  cp "\$UPDATE_DIR/SHA256SUMS.txt" "\$QUORUM_DIR/SHA256SUMS.txt"
fi

rm -rf ~/quorum_update_tmp ~/"\$ZIP"

echo "[server] starting Quorum..."
cd "\$QUORUM_DIR"
bash scripts/run.sh

echo "[server] done!"
REMOTE

# ─── 8. health check ──────────────────────────────────────────────────────────
step "Health check (waiting 5 s)..."
sleep 5
HOST="${SERVER##*@}"   # strip user@ prefix
if curl -sf "http://${HOST}:8080/api/health" > /dev/null 2>&1; then
  echo -e "${GREEN}✓ Backend is responding${NC}"
else
  warn "Backend not responding yet — check with: ssh $SERVER 'tail -50 $REMOTE_DIR/run/backend.log'"
fi

echo -e "\n${GREEN}✓ Deploy complete!${NC}"
echo "  Dashboard: http://${HOST}:8080"
echo "  Zip saved: $ZIP_NAME"
