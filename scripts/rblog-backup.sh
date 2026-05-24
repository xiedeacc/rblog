#!/usr/bin/env bash
set -euo pipefail

BACKUP_REPO_URL="${RBLOG_BACKUP_REPO_URL:-git@github.com:xiedeacc/blog_data.git}"
MAX_FILE_BYTES="${RBLOG_BACKUP_MAX_FILE_BYTES:-52428800}"
SPLIT_BYTES="${RBLOG_BACKUP_SPLIT_BYTES:-49000000}"

script_path="$(readlink -f "${BASH_SOURCE[0]}")"
script_dir="$(dirname "$script_path")"
install_dir="${RBLOG_BACKUP_ROOT:-$(dirname "$script_dir")}"
work_dir="${RBLOG_BACKUP_WORK_DIR:-${install_dir}/.backup-worktree}"
work_dir_name="$(basename "$work_dir")"

log() {
  printf '[rblog-backup] %s\n' "$*"
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'required command not found: %s\n' "$1" >&2
    exit 127
  fi
}

checkout_main_branch() {
  if git -C "$work_dir" show-ref --verify --quiet refs/remotes/origin/main; then
    git -C "$work_dir" checkout -B main origin/main >/dev/null
  else
    git -C "$work_dir" checkout -B main >/dev/null
  fi
}

split_file() {
  local file="$1"
  local split_bytes="$2"
  python3 - "$file" "$split_bytes" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
split_bytes = int(sys.argv[2])
with path.open("rb") as src:
    idx = 0
    while True:
        chunk = src.read(split_bytes)
        if not chunk:
            break
        path.with_name(f"{path.name}.{idx}").write_bytes(chunk)
        idx += 1
PY
}

ensure_repo() {
  if [[ -d "$work_dir/.git" ]]; then
    git -C "$work_dir" remote set-url origin "$BACKUP_REPO_URL"
    git -C "$work_dir" fetch origin main >/dev/null 2>&1 || true
    checkout_main_branch
    git -C "$work_dir" pull --ff-only origin main >/dev/null 2>&1 || true
    return
  fi

  rm -rf "$work_dir"
  if git clone "$BACKUP_REPO_URL" "$work_dir"; then
    checkout_main_branch
    return
  fi

  log "clone failed; initializing a new repository at $work_dir"
  mkdir -p "$work_dir"
  git -C "$work_dir" init -b main
  git -C "$work_dir" remote add origin "$BACKUP_REPO_URL"
}

sync_source() {
  rsync -a --delete \
    --exclude '/logs/' \
    --exclude '/logs/**' \
    --exclude "/${work_dir_name}/" \
    --exclude "/${work_dir_name}/**" \
    --exclude '/.git/' \
    --exclude '/.git/**' \
    "$install_dir/" "$work_dir/"
}

reset_generated_split_files() {
  while IFS= read -r -d '' marker; do
    local dir base original
    dir="$(dirname "$marker")"
    base="$(basename "$marker" .rblog-split)"
    original="$dir/$base"
    rm -f "$original"
    rm -f "$dir/$base".[0-9]*
    rm -f "$marker"
  done < <(find "$work_dir" -name '*.rblog-split' -print0)
}

ignore_path() {
  local rel="$1"
  local ignore="$work_dir/.gitignore"
  touch "$ignore"
  if ! grep -Fxq "$rel" "$ignore"; then
    printf '%s\n' "$rel" >>"$ignore"
  fi
}

split_large_files() {
  local file rel marker
  while IFS= read -r -d '' file; do
    [[ "$file" == "$work_dir/.git/"* ]] && continue
    rel="${file#"$work_dir"/}"
    [[ "$rel" == ".gitignore" ]] && continue
    [[ "$rel" == *.rblog-split ]] && continue
    [[ "$rel" =~ \.[0-9]+$ ]] && continue

    if [[ "$(stat -c '%s' "$file")" -le "$MAX_FILE_BYTES" ]]; then
      continue
    fi

    log "splitting large file: $rel"
    ignore_path "$rel"
    marker="$file.rblog-split"
    rm -f "$file".[0-9]*
    split_file "$file" "$SPLIT_BYTES"
    printf 'original=%s\nsplit_bytes=%s\n' "$rel" "$SPLIT_BYTES" >"$marker"
    rm -f "$file"
  done < <(find "$work_dir" -type f -print0)
}

commit_and_push_if_changed() {
  git -C "$work_dir" add -A
  if git -C "$work_dir" diff --cached --quiet; then
    log "no changes detected"
    if git -C "$work_dir" rev-parse --verify HEAD >/dev/null 2>&1; then
      git -C "$work_dir" push origin main
    fi
    return
  fi

  if ! git -C "$work_dir" config user.email >/dev/null; then
    git -C "$work_dir" config user.email "rblog-backup@localhost"
  fi
  if ! git -C "$work_dir" config user.name >/dev/null; then
    git -C "$work_dir" config user.name "rblog backup"
  fi

  git -C "$work_dir" commit -m "Backup $(date -u +'%Y-%m-%dT%H:%M:%SZ')"
  git -C "$work_dir" push origin main
}

main() {
  require_command git
  require_command rsync
  require_command python3
  require_command stat
  require_command find

  if [[ ! -d "$install_dir" ]]; then
    printf 'install directory not found: %s\n' "$install_dir" >&2
    exit 1
  fi

  log "source: $install_dir"
  log "worktree: $work_dir"
  ensure_repo
  reset_generated_split_files
  sync_source
  split_large_files
  commit_and_push_if_changed
}

main "$@"
