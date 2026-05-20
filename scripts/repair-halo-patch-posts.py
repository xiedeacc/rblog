#!/usr/bin/env python3
"""Repair posts whose content was imported as Halo patch JSON.

Halo stores non-base snapshots as line-based patch arrays in
Snapshot.spec.rawPatch/contentPatch. This script reads the old MySQL dump's
`extensions` table, composes base + head snapshots, and rewrites affected rows
in the clean SQLite `posts` table with materialized markdown/html content.
"""

from __future__ import annotations

import argparse
import json
import sqlite3
from pathlib import Path
from typing import Any


def mysql_unescape(value: str) -> str:
    out: list[str] = []
    i = 0
    escapes = {
        "0": "\0",
        "'": "'",
        '"': '"',
        "b": "\b",
        "n": "\n",
        "r": "\r",
        "t": "\t",
        "Z": "\x1a",
        "\\": "\\",
    }
    while i < len(value):
        ch = value[i]
        if ch == "\\" and i + 1 < len(value):
            i += 1
            out.append(escapes.get(value[i], value[i]))
        else:
            out.append(ch)
        i += 1
    return "".join(out)


def parse_string(sql: str, pos: int) -> tuple[str, int]:
    assert sql[pos] == "'"
    pos += 1
    raw: list[str] = []
    while pos < len(sql):
        ch = sql[pos]
        if ch == "\\" and pos + 1 < len(sql):
            raw.append(ch)
            raw.append(sql[pos + 1])
            pos += 2
            continue
        if ch == "'":
            return mysql_unescape("".join(raw)), pos + 1
        raw.append(ch)
        pos += 1
    raise ValueError("unterminated SQL string")


def parse_value(sql: str, pos: int) -> tuple[Any, int]:
    while pos < len(sql) and sql[pos].isspace():
        pos += 1
    if sql.startswith("_binary", pos):
        pos += len("_binary")
        while pos < len(sql) and sql[pos].isspace():
            pos += 1
    if pos < len(sql) and sql[pos] == "'":
        return parse_string(sql, pos)
    end = pos
    while end < len(sql) and sql[end] not in ",)":
        end += 1
    raw = sql[pos:end].strip()
    if raw.upper() == "NULL":
        return None, end
    return int(raw), end


def iter_extension_rows(dump: Path):
    marker = "INSERT INTO `extensions` VALUES "
    with dump.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if not line.startswith(marker):
                continue
            sql = line[len(marker) :].rstrip()
            pos = 0
            while pos < len(sql):
                while pos < len(sql) and sql[pos] in " \t\r\n,":
                    pos += 1
                if pos >= len(sql) or sql[pos] == ";":
                    break
                if sql[pos] != "(":
                    raise ValueError(f"expected row tuple at offset {pos}")
                pos += 1
                name, pos = parse_value(sql, pos)
                if sql[pos] != ",":
                    raise ValueError(f"expected comma after name at offset {pos}")
                data, pos = parse_value(sql, pos + 1)
                if sql[pos] != ",":
                    raise ValueError(f"expected comma after data at offset {pos}")
                _version, pos = parse_value(sql, pos + 1)
                if sql[pos] != ")":
                    raise ValueError(f"expected tuple close at offset {pos}")
                pos += 1
                yield name, data
            return
    raise ValueError("dump does not contain INSERT data for `extensions`")


def load_content_extensions(dump: Path) -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    prefixes = (
        "/registry/content.halo.run/posts/",
        "/registry/content.halo.run/snapshots/",
    )
    for name, data in iter_extension_rows(dump):
        if not isinstance(name, str) or not name.startswith(prefixes):
            continue
        if not isinstance(data, str):
            continue
        obj = json.loads(data)
        kind = obj.get("kind")
        if kind in {"Post", "Snapshot"}:
            out[name] = obj
    return out


def is_patch_json(value: str) -> bool:
    value = value.strip()
    if not value.startswith("[{"):
        return False
    try:
        data = json.loads(value)
    except json.JSONDecodeError:
        return False
    return isinstance(data, list) and all(
        isinstance(item, dict)
        and "source" in item
        and "target" in item
        and "type" in item
        for item in data
    )


def apply_patch(original: str, patch_json: str) -> str:
    if not patch_json.strip():
        return original
    deltas = json.loads(patch_json)
    if not isinstance(deltas, list):
        return patch_json
    original_lines = [] if original == "" else original.split("\n")
    output: list[str] = []
    cursor = 0
    for delta in sorted(deltas, key=lambda item: item["source"]["position"]):
        source = delta["source"]
        target = delta["target"]
        pos = int(source["position"])
        kind = delta["type"]
        if pos > len(original_lines):
            raise ValueError(f"patch position {pos} beyond base length {len(original_lines)}")
        while cursor < pos:
            output.append(original_lines[cursor])
            cursor += 1
        if kind == "INSERT":
            output.extend(target.get("lines") or [])
        elif kind == "DELETE":
            cursor += len(source.get("lines") or [])
        elif kind == "CHANGE":
            cursor += len(source.get("lines") or [])
            output.extend(target.get("lines") or [])
        else:
            raise ValueError(f"unknown delta type {kind}")
    output.extend(original_lines[cursor:])
    return "\n".join(output)


def compose_content(post: dict[str, Any], extensions: dict[str, dict[str, Any]]) -> tuple[str, str, str]:
    spec = post.get("spec") or {}
    head_name = spec.get("headSnapshot") or spec.get("releaseSnapshot") or spec.get("baseSnapshot")
    base_name = spec.get("baseSnapshot")
    if not head_name or not base_name:
        raise ValueError("post is missing head/base snapshot")
    head = extensions[f"/registry/content.halo.run/snapshots/{head_name}"]
    base = extensions[f"/registry/content.halo.run/snapshots/{base_name}"]
    head_spec = head.get("spec") or {}
    base_spec = base.get("spec") or {}
    raw_type = head_spec.get("rawType") or base_spec.get("rawType") or "markdown"
    base_raw = base_spec.get("rawPatch") or ""
    base_html = base_spec.get("contentPatch") or ""
    if head_name == base_name:
        return base_raw, base_html, raw_type
    raw = apply_patch(base_raw, head_spec.get("rawPatch") or "")
    html = apply_patch(base_html, head_spec.get("contentPatch") or "")
    return raw, html, raw_type


def materialize_target_lines(patch_json: str) -> str:
    deltas = json.loads(patch_json)
    if not isinstance(deltas, list):
        return patch_json
    output: list[str] = []
    for delta in sorted(deltas, key=lambda item: item["target"]["position"]):
        lines = delta.get("target", {}).get("lines") or []
        output.extend(str(line) for line in lines)
    return "\n".join(output)


def looks_like_html(value: str) -> bool:
    trimmed = value.lstrip()
    return trimmed.startswith("<") and any(tag in trimmed[:500].lower() for tag in ("<p", "<h", "<pre", "<ol", "<ul", "<table"))


def excerpt_from_post(post: dict[str, Any], markdown: str) -> str:
    status_excerpt = ((post.get("status") or {}).get("excerpt") or "").strip()
    if status_excerpt:
        return status_excerpt[:180]
    text = " ".join(markdown.replace("#", " ").replace("*", " ").split())
    return text[:180]


def repair(sqlite_db: Path, dump: Path, dry_run: bool) -> int:
    extensions = load_content_extensions(dump)
    conn = sqlite3.connect(sqlite_db)
    rows = conn.execute(
        "SELECT name, title, markdown, html FROM posts WHERE TRIM(markdown) LIKE '[{%' OR TRIM(html) LIKE '[{%'"
    ).fetchall()
    repaired = 0
    for name, title, markdown, html in rows:
        if not is_patch_json(markdown) and not is_patch_json(html):
            continue
        post_key = f"/registry/content.halo.run/posts/{name}"
        post = extensions.get(post_key)
        if not post:
            print(f"skip {title!r}: missing post in dump")
            continue
        try:
            new_markdown, new_html, raw_type = compose_content(post, extensions)
        except Exception as exc:  # noqa: BLE001 - repair tool should continue.
            new_markdown = materialize_target_lines(markdown) if is_patch_json(markdown) else markdown
            new_html = materialize_target_lines(html) if is_patch_json(html) else html
            raw_type = "html" if looks_like_html(new_markdown) else "markdown"
            print(f"fallback {title!r}: {exc}")
        if is_patch_json(new_markdown) or is_patch_json(new_html):
            print(f"skip {title!r}: composed content is still patch JSON")
            continue
        excerpt = excerpt_from_post(post, new_markdown)
        print(f"repair {title!r}: markdown {len(markdown)} -> {len(new_markdown)}, html {len(html)} -> {len(new_html)}")
        if not dry_run:
            conn.execute(
                """
                UPDATE posts
                SET markdown = ?, html = ?, raw_type = ?, excerpt = CASE
                    WHEN excerpt IS NULL OR TRIM(excerpt) = '' THEN ?
                    ELSE excerpt
                END
                WHERE name = ?
                """,
                (new_markdown, new_html, raw_type, excerpt, name),
            )
        repaired += 1
    if dry_run:
        conn.rollback()
    else:
        conn.commit()
    conn.close()
    return repaired


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sqlite-db", required=True, type=Path)
    parser.add_argument("--mysql-dump", required=True, type=Path)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    repaired = repair(args.sqlite_db, args.mysql_dump, args.dry_run)
    print(f"repaired={repaired}")


if __name__ == "__main__":
    main()
