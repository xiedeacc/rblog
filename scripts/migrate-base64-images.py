#!/usr/bin/env python3
"""Move base64 data URI images from Halo snapshots into local uploads."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import re
import sqlite3
from pathlib import Path


DATA_IMAGE_RE = re.compile(
    r"data:image/(?P<mime>[A-Za-z0-9.+-]+);base64,(?P<data>[A-Za-z0-9+/=\r\n]+)"
)

EXTENSIONS = {
    "jpeg": "jpg",
    "jpg": "jpg",
    "png": "png",
    "gif": "gif",
    "webp": "webp",
    "svg+xml": "svg",
    "bmp": "bmp",
}


def extension_for(mime: str) -> str:
    return EXTENSIONS.get(mime.lower(), mime.lower().split("+", 1)[0])


def migrate_text(
    text: str,
    uploads_root: Path,
    public_prefix: str,
    dry_run: bool,
    stats: dict[str, int],
) -> str:
    def replace(match: re.Match[str]) -> str:
        mime = match.group("mime")
        payload = re.sub(r"\s+", "", match.group("data"))
        try:
            image = base64.b64decode(payload, validate=True)
        except ValueError:
            stats["invalid"] += 1
            return match.group(0)
        if not image:
            stats["invalid"] += 1
            return match.group(0)

        digest = hashlib.sha256(image).hexdigest()
        ext = extension_for(mime)
        key = f"base64-migrated/{digest}.{ext}"
        target = uploads_root / key
        if not target.exists():
            stats["files_created"] += 1
            if not dry_run:
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(image)
        else:
            stats["files_reused"] += 1
        stats["images_replaced"] += 1
        return f"{public_prefix.rstrip('/')}/{key}"

    return DATA_IMAGE_RE.sub(replace, text)


def migrate_snapshot(
    obj: dict,
    uploads_root: Path,
    public_prefix: str,
    dry_run: bool,
    stats: dict[str, int],
) -> tuple[dict, bool]:
    changed = False
    spec = obj.get("spec")
    if not isinstance(spec, dict):
        return obj, False

    for field in ("rawPatch", "contentPatch"):
        value = spec.get(field)
        if isinstance(value, str) and "data:image/" in value:
            updated = migrate_text(value, uploads_root, public_prefix, dry_run, stats)
            if updated != value:
                spec[field] = updated
                changed = True

    return obj, changed


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--db", default="/usr/local/blog/data/rblog.db")
    parser.add_argument("--uploads-root", default="/usr/local/blog/data/uploads")
    parser.add_argument("--public-prefix", default="/uploads")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    db_path = Path(args.db)
    uploads_root = Path(args.uploads_root)
    stats = {
        "snapshots_scanned": 0,
        "snapshots_changed": 0,
        "images_replaced": 0,
        "files_created": 0,
        "files_reused": 0,
        "invalid": 0,
    }

    conn = sqlite3.connect(db_path)
    try:
        rows = conn.execute(
            """
            SELECT name, data, version
            FROM extensions
            WHERE name LIKE '/registry/content.halo.run/snapshots/%'
              AND CAST(data AS TEXT) LIKE '%data:image/%'
            ORDER BY name
            """
        ).fetchall()

        with conn:
            for name, data, version in rows:
                stats["snapshots_scanned"] += 1
                obj = json.loads(data)
                updated, changed = migrate_snapshot(
                    obj,
                    uploads_root,
                    args.public_prefix,
                    args.dry_run,
                    stats,
                )
                if not changed:
                    continue

                stats["snapshots_changed"] += 1
                if args.dry_run:
                    continue

                metadata = updated.setdefault("metadata", {})
                if isinstance(metadata, dict):
                    metadata["version"] = int(metadata.get("version") or 0) + 1
                encoded = json.dumps(updated, ensure_ascii=False, separators=(",", ":")).encode()
                conn.execute(
                    "UPDATE extensions SET data = ?, version = version + 1 WHERE name = ? AND version = ?",
                    (encoded, name, version),
                )
    finally:
        conn.close()

    for key, value in stats.items():
        print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
