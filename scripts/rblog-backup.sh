#!/usr/bin/env bash
# Mirror an rblog deployment's bin/, conf/, and data/ into a separate Git
# worktree and push it to a private backup repository.
#
# The SQLite database is captured with VACUUM INTO rather than copied off disk,
# so the snapshot is transactionally consistent even while rblog is serving
# writes. A raw copy of a live database can capture a torn page and silently
# produce an unrestorable backup. This mirrors domus-backup / rgit-backup.
set -euo pipefail

BACKUP_REPO_URL="${RBLOG_BACKUP_REPO_URL:-git@github.com:xiedeacc/blog_data.git}"
BACKUP_BRANCH="${RBLOG_BACKUP_BRANCH:-master}"
MAX_FILE_BYTES="${RBLOG_BACKUP_MAX_FILE_BYTES:-52428800}"  # 50 MiB commit threshold
SPLIT_BYTES="${RBLOG_BACKUP_SPLIT_BYTES:-49000000}"         # ~49 MiB chunk size

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
install_dir="${RBLOG_BACKUP_ROOT:-$(dirname "$script_dir")}"
work_dir="${RBLOG_BACKUP_WORK_DIR:-${install_dir}/.backup-worktree}"
db_file="${RBLOG_BACKUP_DB:-${install_dir}/data/rblog.db}"
lock_file="${RBLOG_BACKUP_LOCK_FILE:-${install_dir}/data/.backup.lock}"

log() {
    echo "[rblog-backup] $*"
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        log "required command not found: $1"
        exit 127
    fi
}

# domus/rgit call the sqlite3 CLI directly, but it is not installed on every
# host rblog deploys to. python3 is already a hard dependency here and ships a
# SQLite new enough for VACUUM INTO, so use it when the CLI is absent.
sqlite_exec() {
    local db="$1"
    local sql="$2"
    if command -v sqlite3 >/dev/null 2>&1; then
        sqlite3 "$db" "$sql"
    else
        python3 - "$db" "$sql" <<'PYEOF'
import sqlite3
import sys

con = sqlite3.connect(sys.argv[1])
con.isolation_level = None  # autocommit; VACUUM cannot run inside a transaction
try:
    for row in con.execute(sys.argv[2]):
        print("|".join("" if v is None else str(v) for v in row))
finally:
    con.close()
PYEOF
    fi
}

checkout_backup_branch() {
    if git -C "$work_dir" rev-parse --verify "origin/${BACKUP_BRANCH}" >/dev/null 2>&1; then
        git -C "$work_dir" checkout -B "$BACKUP_BRANCH" "origin/${BACKUP_BRANCH}"
    elif git -C "$work_dir" rev-parse --verify "$BACKUP_BRANCH" >/dev/null 2>&1; then
        git -C "$work_dir" checkout "$BACKUP_BRANCH"
    else
        git -C "$work_dir" checkout --orphan "$BACKUP_BRANCH"
    fi
}

ensure_repo() {
    if [ -d "${work_dir}/.git" ]; then
        git -C "$work_dir" remote set-url origin "$BACKUP_REPO_URL"
        git -C "$work_dir" fetch origin "$BACKUP_BRANCH" || true
        checkout_backup_branch
        git -C "$work_dir" pull --ff-only origin "$BACKUP_BRANCH" || true
        return
    fi

    if git clone --branch "$BACKUP_BRANCH" "$BACKUP_REPO_URL" "$work_dir" 2>/dev/null; then
        return
    fi

    log "clone failed (empty remote?) - initializing local repo"
    mkdir -p "$work_dir"
    git -C "$work_dir" init -b "$BACKUP_BRANCH"
    git -C "$work_dir" remote add origin "$BACKUP_REPO_URL"
}

clear_directory() {
    local dir="$1"
    mkdir -p "$dir"
    find "$dir" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
}

should_exclude_none() {
    return 1
}

should_exclude_bin() {
    case "$1" in
        *.bak|*.bak-*) return 0 ;;
    esac
    return 1
}

# The live database and its journal/WAL sidecars are never copied off disk;
# snapshot_sqlite writes data/rblog.db in the worktree instead.
should_exclude_data() {
    case "$1" in
        .backup.lock|.rblog.db.*.tmp*|rblog.db|rblog.db.tmp|rblog.db.bak|rblog.db-journal|rblog.db-shm|rblog.db-wal) return 0 ;;
    esac
    return 1
}

copy_tree() {
    local src="$1"
    local dst="$2"
    local exclude_func="$3"
    local rel

    clear_directory "$dst"
    (
        cd "$src"
        while IFS= read -r -d '' rel; do
            rel="${rel#./}"
            if "$exclude_func" "$rel"; then
                continue
            fi
            if [ -d "$src/$rel" ] && [ ! -L "$src/$rel" ]; then
                mkdir -p "$dst/$rel"
            else
                mkdir -p "$dst/$(dirname "$rel")"
                cp -a "$src/$rel" "$dst/$rel"
            fi
        done < <(find . -mindepth 1 -print0)
    )
}

sync_source() {
    find "$work_dir" -mindepth 1 -maxdepth 1 \
        ! -name '.git' ! -name 'bin' ! -name 'conf' ! -name 'data' \
        ! -name '.rblog-empty-dirs' -exec rm -rf -- {} +
    mkdir -p "$work_dir/bin" "$work_dir/conf" "$work_dir/data"

    if [ -d "$install_dir/bin" ]; then
        copy_tree "$install_dir/bin" "$work_dir/bin" should_exclude_bin
    fi
    if [ -d "$install_dir/conf" ]; then
        copy_tree "$install_dir/conf" "$work_dir/conf" should_exclude_none
    fi
    if [ -d "$install_dir/data" ]; then
        copy_tree "$install_dir/data" "$work_dir/data" should_exclude_data
    fi
}

snapshot_sqlite() {
    local snapshot="$work_dir/data/rblog.db"
    local temporary="${snapshot}.tmp"
    rm -f "$snapshot" "$temporary"

    if [ ! -f "$db_file" ]; then
        log "no sqlite db at ${db_file} - skipping snapshot"
        return
    fi

    mkdir -p "$work_dir/data"
    sqlite_exec "$db_file" "VACUUM INTO '$temporary'"
    mv "$temporary" "$snapshot"
    log "sqlite snapshot written: $snapshot"

    local integrity
    integrity="$(sqlite_exec "$snapshot" 'PRAGMA integrity_check')"
    if [ "$integrity" != "ok" ]; then
        log "sqlite snapshot integrity check failed: $integrity"
        return 1
    fi
}

record_empty_dirs() {
    local manifest="$work_dir/.rblog-empty-dirs"
    find "$work_dir" -type d -empty \
        -not -path "$work_dir/.git" \
        -not -path "$work_dir/.git/*" \
        -printf '%P\n' | LC_ALL=C sort >"$manifest"
}

split_file() {
    local file="$1"
    python3 - "$file" "$SPLIT_BYTES" <<'PYEOF'
import sys

path, chunk = sys.argv[1], int(sys.argv[2])
i = 0
with open(path, "rb") as f:
    while True:
        data = f.read(chunk)
        if not data:
            break
        with open(f"{path}.{i}", "wb") as out:
            out.write(data)
        i += 1
PYEOF
}

ignore_path() {
    local rel="$1"
    local exclude="$work_dir/.git/info/exclude"
    mkdir -p "$(dirname "$exclude")"
    touch "$exclude"
    if ! grep -qxF "/$rel" "$exclude"; then
        echo "/$rel" >>"$exclude"
    fi
}

reset_generated_split_files() {
    find "$work_dir" -name '*.rblog-split' -not -path "*/.git/*" | while read -r marker; do
        base="${marker%.rblog-split}"
        rm -f "$base" "$base".[0-9]* "$marker"
    done
}

split_large_files() {
    find "$work_dir" -type f -size +"$MAX_FILE_BYTES"c \
        -not -path "*/.git/*" \
        -not -name '.gitignore' \
        -not -name '*.rblog-split' | while read -r file; do
        rel="${file#"$work_dir"/}"
        case "$rel" in
            *.[0-9]|*.[0-9][0-9]) continue ;;
        esac

        log "splitting large file: $rel"
        ignore_path "$rel"
        size="$(stat -c '%s' "$file")"
        digest="$(sha256sum "$file" | cut -d' ' -f1)"
        chunks="$(( (size + SPLIT_BYTES - 1) / SPLIT_BYTES ))"
        split_file "$file"
        {
            echo "original=$rel"
            echo "split_bytes=$SPLIT_BYTES"
            echo "size=$size"
            echo "sha256=$digest"
            echo "chunks=$chunks"
        } >"${file}.rblog-split"
        rm -f "$file"
    done
}

commit_and_push_if_changed() {
    git -C "$work_dir" add -A
    if git -C "$work_dir" diff --cached --quiet; then
        log "no changes to back up"
        if git -C "$work_dir" rev-parse --verify HEAD >/dev/null 2>&1; then
            git -C "$work_dir" pull --ff-only origin "$BACKUP_BRANCH" || true
            git -C "$work_dir" push origin "$BACKUP_BRANCH"
        fi
        return
    fi

    if [ -z "$(git -C "$work_dir" config user.email || true)" ]; then
        git -C "$work_dir" config user.email "rblog-backup@localhost"
        git -C "$work_dir" config user.name "rblog backup"
    fi

    git -C "$work_dir" commit -m "Backup $(date -u +'%Y-%m-%dT%H:%M:%SZ')"
    git -C "$work_dir" pull --rebase origin "$BACKUP_BRANCH" || true
    git -C "$work_dir" push origin "$BACKUP_BRANCH"
    log "backup pushed"
}

main() {
    require_command git
    require_command python3
    require_command find
    require_command stat
    require_command sha256sum
    require_command flock

    if [ ! -d "$install_dir" ]; then
        log "install dir not found: $install_dir"
        exit 1
    fi

    mkdir -p "$(dirname "$lock_file")"
    exec 9>"$lock_file"
    if ! flock -n 9; then
        log "another backup is already running"
        exit 75
    fi

    log "backing up $install_dir/{bin,conf,data} -> $BACKUP_REPO_URL ($BACKUP_BRANCH)"
    ensure_repo
    reset_generated_split_files
    sync_source
    snapshot_sqlite
    record_empty_dirs
    split_large_files
    commit_and_push_if_changed
    log "done"
}

main "$@"
