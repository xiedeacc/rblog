import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Dropdown, Space, Tooltip } from "antd";
import {
  BoldOutlined,
  ItalicOutlined,
  CodeOutlined,
  UnorderedListOutlined,
  OrderedListOutlined,
  StrikethroughOutlined,
  UndoOutlined,
  RedoOutlined,
  ClearOutlined,
  TableOutlined,
  LinkOutlined,
  EyeOutlined,
  PlusOutlined,
} from "@ant-design/icons";
import { marked } from "marked";
import mermaid from "mermaid";
import katex from "katex";
import "katex/dist/katex.min.css";

interface Props {
  initialMarkdown: string;
  onChange: (markdown: string) => void;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function slugify(value: string): string {
  return value
    .toLowerCase()
    .trim()
    .replace(/[^\w\s-]/g, "")
    .replace(/\s+/g, "-");
}

function renderMath(markdown: string): string {
  return markdown
    .replace(/\$\$([\s\S]+?)\$\$/g, (_match, expr: string) =>
      katex.renderToString(expr.trim(), { displayMode: true, throwOnError: false }),
    )
    .replace(/(^|[^\\])\$([^\n$]+?)\$/g, (_match, prefix: string, expr: string) =>
      `${prefix}${katex.renderToString(expr.trim(), { displayMode: false, throwOnError: false })}`,
    );
}

function renderToc(markdown: string, html: string): string {
  const headings = [...markdown.matchAll(/^(#{1,3})\s+(.+)$/gm)].map((match) => ({
    depth: (match[1] ?? "").length,
    title: (match[2] ?? "").replace(/[*_`]/g, "").trim(),
  }));
  if (!headings.length) return html.replace(/\[\[toc\]\]/gi, "");
  const toc = `<nav class="markdown-preview__toc"><strong>Table of contents</strong><ul>${headings
    .map((heading) => `<li class="depth-${heading.depth}"><a href="#${slugify(heading.title)}">${heading.title}</a></li>`)
    .join("")}</ul></nav>`;
  let next = html.replace(/\[\[toc\]\]/gi, toc);
  for (const heading of headings) {
    const text = heading.title;
    next = next.replace(
      new RegExp(`<h${heading.depth}>${escapeRegExp(text)}</h${heading.depth}>`),
      `<h${heading.depth} id="${slugify(text)}">${text}</h${heading.depth}>`,
    );
  }
  return next;
}

function renderPreview(markdown: string): string {
  const withMath = renderMath(markdown);
  const html = marked.parse(withMath, { async: false, gfm: true }) as string;
  return renderToc(markdown, html).replace(
    /<pre><code class="language-mermaid">([\s\S]*?)<\/code><\/pre>/g,
    '<pre class="mermaid">$1</pre>',
  );
}

export function MarkdownEditor({ initialMarkdown, onChange }: Props) {
  const [markdown, setMarkdown] = useState(initialMarkdown);
  const [preview, setPreview] = useState(true);
  const textarea = useRef<HTMLTextAreaElement | null>(null);
  const previewRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    setMarkdown(initialMarkdown);
  }, [initialMarkdown]);

  useEffect(() => {
    mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
  }, []);

  useEffect(() => {
    if (!previewRef.current) return;
    void mermaid.run({ nodes: previewRef.current.querySelectorAll(".mermaid") });
  }, [markdown, preview]);

  const html = useMemo(() => renderPreview(markdown), [markdown]);

  const update = (next: string) => {
    setMarkdown(next);
    onChange(next);
  };

  const insert = (before: string, after = "", placeholder = "") => {
    const el = textarea.current;
    if (!el) return;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const selected = markdown.slice(start, end) || placeholder;
    const next = `${markdown.slice(0, start)}${before}${selected}${after}${markdown.slice(end)}`;
    update(next);
    requestAnimationFrame(() => {
      el.focus();
      el.selectionStart = start + before.length;
      el.selectionEnd = start + before.length + selected.length;
    });
  };

  const clearFormat = () => {
    const el = textarea.current;
    if (!el) return;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const selected = markdown.slice(start, end);
    if (!selected) return;
    const plain = selected
      .replace(/[*_~`>#-]/g, "")
      .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1");
    update(`${markdown.slice(0, start)}${plain}${markdown.slice(end)}`);
  };

  return (
    <div className="markdown-editor-shell">
      <div className="lexical-toolbar">
        <Space size={4} wrap>
          <Tooltip title="Undo"><Button size="small" icon={<UndoOutlined />} onClick={() => document.execCommand("undo")} /></Tooltip>
          <Tooltip title="Redo"><Button size="small" icon={<RedoOutlined />} onClick={() => document.execCommand("redo")} /></Tooltip>
          <Tooltip title="Bold"><Button size="small" icon={<BoldOutlined />} onClick={() => insert("**", "**", "bold")} /></Tooltip>
          <Tooltip title="Italic"><Button size="small" icon={<ItalicOutlined />} onClick={() => insert("_", "_", "italic")} /></Tooltip>
          <Tooltip title="Strikethrough"><Button size="small" icon={<StrikethroughOutlined />} onClick={() => insert("~~", "~~", "deleted")} /></Tooltip>
          <Tooltip title="Inline code"><Button size="small" icon={<CodeOutlined />} onClick={() => insert("`", "`", "code")} /></Tooltip>
          <Tooltip title="Bullet list"><Button size="small" icon={<UnorderedListOutlined />} onClick={() => insert("- ", "", "list item")} /></Tooltip>
          <Tooltip title="Numbered list"><Button size="small" icon={<OrderedListOutlined />} onClick={() => insert("1. ", "", "list item")} /></Tooltip>
          <Tooltip title="Link"><Button size="small" icon={<LinkOutlined />} onClick={() => insert("[", "](https://)", "link text")} /></Tooltip>
          <Tooltip title="Table"><Button size="small" icon={<TableOutlined />} onClick={() => insert("\n| Column | Column |\n| --- | --- |\n| ", " | value |\n", "value")} /></Tooltip>
          <Tooltip title="Clear format"><Button size="small" icon={<ClearOutlined />} onClick={clearFormat} /></Tooltip>
          <Dropdown
            menu={{
              items: [
                { key: "toc", label: "TOC", onClick: () => insert("\n[[toc]]\n") },
                { key: "detail", label: "Detail block", onClick: () => insert("\n<details>\n<summary>", "</summary>\n\nDetail content\n</details>\n", "Title") },
                { key: "mermaid", label: "Mermaid", onClick: () => insert("\n```mermaid\ngraph TD\n  A[Start] --> B[End]\n```\n") },
                { key: "math", label: "Math formula", onClick: () => insert("\n$$\n", "\n$$\n", "E = mc^2") },
                { key: "code", label: "Code block", onClick: () => insert("\n```text\n", "\n```\n", "code") },
              ],
            }}
          >
            <Button size="small" icon={<PlusOutlined />}>Insert</Button>
          </Dropdown>
          <Button size="small" icon={<EyeOutlined />} onClick={() => setPreview((value) => !value)}>
            {preview ? "Hide preview" : "Preview"}
          </Button>
        </Space>
      </div>
      <div className={`markdown-editor-grid${preview ? "" : " markdown-editor-grid--single"}`}>
        <textarea
          ref={textarea}
          className="markdown-editor-input"
          aria-label="Post body"
          value={markdown}
          onChange={(event) => update(event.target.value)}
          placeholder="Write your post in markdown..."
        />
        {preview ? (
          <div
            ref={previewRef}
            className="markdown-preview"
            dangerouslySetInnerHTML={{ __html: html }}
          />
        ) : null}
      </div>
    </div>
  );
}
