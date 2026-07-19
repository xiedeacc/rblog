#!/usr/bin/env bash
set -euo pipefail

APP_NAME="rblog"
SERVICE_NAME="rblog"
PORT="10011"
DEST_DIR="/usr/local/blog"
REMOTE_HOST="ubuntu@aws"
BACKUP_REPO_URL="git@github.com:xiedeacc/blog_data.git"
NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
NODE_VERSION="${NODE_VERSION:-24}"

usage() {
  printf 'Usage: %s [local|aws|all]\n' "$0"
  printf '  local  Build and deploy to this host\n'
  printf '  aws    Build and deploy to %s\n' "$REMOTE_HOST"
  printf '  all    Deploy to local and aws (default)\n'
}

TARGET="${1:-all}"
case "$TARGET" in
  local|aws|all) ;;
  -h|--help) usage; exit 0 ;;
  *) usage; exit 2 ;;
esac

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="$(mktemp -d)"
trap 'rm -rf "$BUILD_DIR"' EXIT

log() {
  printf '[deploy] %s\n' "$*"
}

run_remote() {
  local host="$1"
  shift
  ssh "$host" "$@"
}

load_node_env() {
  if command -v corepack >/dev/null 2>&1; then
    return
  fi
  if [[ -s "$NVM_DIR/nvm.sh" ]]; then
    # shellcheck source=/dev/null
    . "$NVM_DIR/nvm.sh"
    nvm use "$NODE_VERSION" >/dev/null || nvm use default >/dev/null || true
  fi
  command -v corepack >/dev/null 2>&1 || {
    printf '[deploy] ERROR: corepack not found; install Node via nvm or set NODE_VERSION to an installed nvm version\n' >&2
    exit 127
  }
}

copy_dir_local() {
  local src="$1"
  local dst="$2"
  local delete="${3:-0}"
  sudo mkdir -p "$dst"
  if [[ "$delete" == "1" ]]; then
    sudo find "$dst" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
  fi
  sudo cp -a "$src/." "$dst/"
}

copy_dir_remote() {
  local host="$1"
  local src="$2"
  local dst="$3"
  local delete="${4:-0}"
  run_remote "$host" "sudo mkdir -p '$dst'"
  if [[ "$delete" == "1" ]]; then
    run_remote "$host" "sudo find '$dst' -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +"
  fi
  tar -C "$src" -cf - . | ssh "$host" "sudo tar -C '$dst' -xf -"
}

sudo_write_remote() {
  local host="$1"
  local dst="$2"
  ssh "$host" "sudo tee '$dst' >/dev/null"
}

write_config() {
  local dst="$1"
  local secure_cookie="${2:-false}"
  mkdir -p "$(dirname "$dst")"
  cat >"$dst" <<EOF_CONFIG
[server]
bind = "127.0.0.1:${PORT}"
max_body_mb = 16
request_timeout_seconds = 30

[database]
url = "sqlite:${DEST_DIR}/data/rblog.db?mode=rwc"

[paths]
themes_root = "${DEST_DIR}/data/themes"
uploads_root = "${DEST_DIR}/data/uploads"
search_root = "${DEST_DIR}/data/search-index"
plugins_root = "${DEST_DIR}/data/plugins"
admin_dist = "${DEST_DIR}/bin/admin"

[session]
cookie_name = "rblog_session"
secure = ${secure_cookie}
max_age_days = 14

[storage]
backend = "local"
public_prefix = "/uploads"
EOF_CONFIG
}

write_service() {
  local dst="$1"
  local run_user="$2"
  mkdir -p "$(dirname "$dst")"
  cat >"$dst" <<EOF_SERVICE
[Unit]
Description=rblog blog service
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${run_user}
WorkingDirectory=${DEST_DIR}/data
Environment=RBLOG_CONFIG=${DEST_DIR}/conf/rblog.toml
Environment=RUST_LOG=info,sqlx=warn
ExecStart=${DEST_DIR}/bin/${APP_NAME}
Restart=always
RestartSec=5
StandardOutput=append:${DEST_DIR}/logs/${APP_NAME}.log
StandardError=append:${DEST_DIR}/logs/${APP_NAME}.log

[Install]
WantedBy=multi-user.target
EOF_SERVICE
}

write_backup_service() {
  local dst="$1"
  local run_user="$2"
  mkdir -p "$(dirname "$dst")"
  cat >"$dst" <<EOF_SERVICE
[Unit]
Description=rblog data backup
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
User=${run_user}
WorkingDirectory=${DEST_DIR}
Environment=RBLOG_BACKUP_REPO_URL=${BACKUP_REPO_URL}
Environment=RBLOG_BACKUP_WORK_DIR=${DEST_DIR}/.backup-worktree
ExecStart=${DEST_DIR}/bin/rblog-backup
EOF_SERVICE
}

write_backup_timer() {
  local dst="$1"
  mkdir -p "$(dirname "$dst")"
  cat >"$dst" <<EOF_TIMER
[Unit]
Description=Run rblog data backup every hour

[Timer]
OnBootSec=5min
OnUnitActiveSec=1h
AccuracySec=1min
Persistent=true
Unit=rblog-backup.service

[Install]
WantedBy=timers.target
EOF_TIMER
}

build_artifacts() {
  load_node_env

  log "building admin SPA"
  (cd "$ROOT_DIR" && corepack pnpm --dir admin build)

  log "building release binary"
  (cd "$ROOT_DIR" && cargo build --release)

  log "staging artifacts"
  mkdir -p "$BUILD_DIR/bin/admin" "$BUILD_DIR/conf" "$BUILD_DIR/data/themes/default"
  cp "$ROOT_DIR/target/release/${APP_NAME}" "$BUILD_DIR/bin/${APP_NAME}"
  cp "$ROOT_DIR/scripts/rblog-backup.sh" "$BUILD_DIR/bin/rblog-backup"
  chmod +x "$BUILD_DIR/bin/rblog-backup"
  cp -R "$ROOT_DIR/admin/dist/." "$BUILD_DIR/bin/admin/"
  cp -R "$ROOT_DIR/crates/rblog-theme/default/." "$BUILD_DIR/data/themes/default/"
  write_config "$BUILD_DIR/conf/rblog.toml" false
}

stage_binary_for_arch() {
  local arch="$1"
  case "$arch" in
    x86_64|amd64)
      cp "$ROOT_DIR/target/release/${APP_NAME}" "$BUILD_DIR/bin/${APP_NAME}"
      ;;
    aarch64|arm64)
      log "building aarch64 release binary for remote host"
      (
        cd "$ROOT_DIR"
        CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER:-aarch64-linux-gnu-gcc}" \
          cargo build --release --target aarch64-unknown-linux-gnu
      )
      cp "$ROOT_DIR/target/aarch64-unknown-linux-gnu/release/${APP_NAME}" "$BUILD_DIR/bin/${APP_NAME}"
      ;;
    *)
      printf 'Unsupported remote architecture: %s\n' "$arch" >&2
      exit 3
      ;;
  esac
}

deploy_local() {
  local run_user="${USER:-$(id -un)}"
  local service_file="$BUILD_DIR/${SERVICE_NAME}.service"
  local backup_service_file="$BUILD_DIR/${SERVICE_NAME}-backup.service"
  local backup_timer_file="$BUILD_DIR/${SERVICE_NAME}-backup.timer"
  write_service "$service_file" "$run_user"
  write_backup_service "$backup_service_file" "$run_user"
  write_backup_timer "$backup_timer_file"

  log "deploying locally to ${DEST_DIR}"
  sudo mkdir -p \
    "$DEST_DIR/bin" \
    "$DEST_DIR/conf" \
    "$DEST_DIR/logs" \
    "$DEST_DIR/data/themes" \
    "$DEST_DIR/data/uploads" \
    "$DEST_DIR/data/search-index" \
    "$DEST_DIR/data/plugins"
  copy_dir_local "$BUILD_DIR/bin" "$DEST_DIR/bin" 1
  copy_dir_local "$BUILD_DIR/conf" "$DEST_DIR/conf" 0
  copy_dir_local "$BUILD_DIR/data/themes" "$DEST_DIR/data/themes" 1
  sudo cp "$service_file" "/etc/systemd/system/${SERVICE_NAME}.service"
  sudo cp "$backup_service_file" "/etc/systemd/system/${SERVICE_NAME}-backup.service"
  sudo cp "$backup_timer_file" "/etc/systemd/system/${SERVICE_NAME}-backup.timer"
  sudo chown -R "$run_user:$run_user" "$DEST_DIR"
  sudo systemctl daemon-reload
  sudo systemctl enable "$SERVICE_NAME"
  sudo systemctl enable --now "${SERVICE_NAME}-backup.timer"
  sudo systemctl restart "$SERVICE_NAME"
  sudo systemctl --no-pager --full status "$SERVICE_NAME"
}

deploy_remote() {
  local host="$1"
  local remote_user="${host%@*}"
  local service_file="$BUILD_DIR/${SERVICE_NAME}.service"
  local backup_service_file="$BUILD_DIR/${SERVICE_NAME}-backup.service"
  local backup_timer_file="$BUILD_DIR/${SERVICE_NAME}-backup.timer"
  local remote_arch
  remote_arch="$(ssh "$host" "uname -m")"
  stage_binary_for_arch "$remote_arch"
  write_config "$BUILD_DIR/conf/rblog.toml" true
  write_service "$service_file" "$remote_user"
  write_backup_service "$backup_service_file" "$remote_user"
  write_backup_timer "$backup_timer_file"

  log "deploying to ${host}:${DEST_DIR} (${remote_arch})"
  run_remote "$host" "sudo mkdir -p '$DEST_DIR/bin' '$DEST_DIR/conf' '$DEST_DIR/logs' '$DEST_DIR/data/themes' '$DEST_DIR/data/uploads' '$DEST_DIR/data/search-index' '$DEST_DIR/data/plugins' && sudo chown -R '$remote_user:$remote_user' '$DEST_DIR'"
  copy_dir_remote "$host" "$BUILD_DIR/bin" "$DEST_DIR/bin" 1
  copy_dir_remote "$host" "$BUILD_DIR/conf" "$DEST_DIR/conf" 0
  copy_dir_remote "$host" "$BUILD_DIR/data/themes" "$DEST_DIR/data/themes" 1
  sudo_write_remote "$host" "/etc/systemd/system/${SERVICE_NAME}.service" <"$service_file"
  sudo_write_remote "$host" "/etc/systemd/system/${SERVICE_NAME}-backup.service" <"$backup_service_file"
  sudo_write_remote "$host" "/etc/systemd/system/${SERVICE_NAME}-backup.timer" <"$backup_timer_file"
  run_remote "$host" "sudo chown -R '$remote_user:$remote_user' '$DEST_DIR' && sudo systemctl daemon-reload && sudo systemctl enable '$SERVICE_NAME' && sudo systemctl enable --now '${SERVICE_NAME}-backup.timer' && sudo systemctl restart '$SERVICE_NAME' && sudo systemctl --no-pager --full status '$SERVICE_NAME' && sudo systemctl --no-pager --full status '${SERVICE_NAME}-backup.timer'"
}

build_artifacts

if [[ "$TARGET" == "local" || "$TARGET" == "all" ]]; then
  deploy_local
fi

if [[ "$TARGET" == "aws" || "$TARGET" == "all" ]]; then
  deploy_remote "$REMOTE_HOST"
fi

log "done. service listens on port ${PORT}"
