import { useEffect, useMemo, useRef, useState, type ClipboardEvent, type ReactNode, type RefObject, type UIEvent } from "react";
import { App, Button, Dropdown, Space, Tooltip } from "antd";
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
  UploadOutlined,
  PictureOutlined,
  VideoCameraOutlined,
  AudioOutlined,
} from "@ant-design/icons";
import { marked } from "marked";
import mermaid from "mermaid";
import katex from "katex";
import "katex/dist/katex.min.css";
import { uploadAttachment } from "@/api/client";

interface Props {
  initialMarkdown: string;
  onChange: (markdown: string) => void;
  stickyHeader?: ReactNode;
}

interface HeadingItem {
  depth: number;
  title: string;
  id: string;
  line: number;
  offset: number;
}

interface HeadingPosition {
  id: string;
  top: number;
}

interface SourceBlock {
  line: number;
  offset: number;
}

interface MarkdownPreviewProps {
  markdown: string;
  className?: string;
  previewRef?: RefObject<HTMLDivElement | null>;
  onScroll?: (event: UIEvent<HTMLDivElement>) => void;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function slugify(value: string): string {
  const slug = value
    .toLowerCase()
    .trim()
    .replace(/[^\p{L}\p{N}\s-]/gu, "")
    .replace(/\s+/g, "-");
  return slug || "section";
}

function lineAtOffset(value: string, offset: number): number {
  return value.slice(0, offset).split("\n").length - 1;
}

function extractHeadings(markdown: string): HeadingItem[] {
  const headings: HeadingItem[] = [];
  const seen = new Map<string, number>();
  const addHeading = (depth: number, rawTitle: string, offset: number) => {
    const title = rawTitle
      .replace(/<[^>]+>/g, "")
      .replace(/&nbsp;/g, " ")
      .replace(/&amp;/g, "&")
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/[*_`]/g, "")
      .trim();
    if (!title) return;
    const baseId = slugify(title);
    const count = seen.get(baseId) ?? 0;
    seen.set(baseId, count + 1);
    headings.push({
      depth,
      title,
      id: count === 0 ? baseId : `${baseId}-${count + 1}`,
      line: lineAtOffset(markdown, offset),
      offset,
    });
  };

  let inFence = false;
  for (const match of markdown.matchAll(/^.*$/gm)) {
    const line = match[0] ?? "";
    const offset = match.index ?? 0;
    if (/^\s*(```|~~~)/.test(line)) {
      inFence = !inFence;
      continue;
    }
    if (inFence) continue;
    const markdownHeading = /^(#{1,6})[ \t]+(.+?)(?:[ \t]+#+)?$/.exec(line);
    if (markdownHeading) {
      addHeading((markdownHeading[1] ?? "").length, markdownHeading[2] ?? "", offset);
      continue;
    }
    const htmlHeading = /<h([1-6])(?:\s[^>]*)?>([\s\S]*?)<\/h\1>/i.exec(line);
    if (htmlHeading) {
      addHeading(Number(htmlHeading[1]), htmlHeading[2] ?? "", offset);
    }
  }
  return headings.sort((a, b) => a.offset - b.offset);
}

function extractSourceBlocks(markdown: string): SourceBlock[] {
  const blocks: SourceBlock[] = [];
  let offset = 0;
  let previousBlank = true;
  let inFence = false;
  const lines = markdown.split("\n");
  lines.forEach((line, lineIndex) => {
    const trimmed = line.trim();
    const startsFence = /^\s*(```|~~~)/.test(line);
    if (!trimmed) {
      previousBlank = true;
      offset += line.length + 1;
      return;
    }

    const startsBlock =
      previousBlank ||
      startsFence ||
      /^(#{1,6})\s+/.test(line) ||
      /^\s*(?:[-*+]|\d+\.)\s+/.test(line) ||
      /^\s*>\s?/.test(line) ||
      /^\s*\|.+\|\s*$/.test(line) ||
      /^\s*<h[1-6](?:\s[^>]*)?>/i.test(line);

    if (startsBlock) {
      blocks.push({ line: lineIndex, offset });
    }

    if (startsFence) {
      inFence = !inFence;
    }
    previousBlank = false;
    if (inFence && !startsFence) {
      previousBlank = false;
    }
    offset += line.length + 1;
  });
  return blocks;
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
  const headings = extractHeadings(markdown);
  if (!headings.length) return html.replace(/\[\[toc\]\]/gi, "");
  const toc = `<nav class="markdown-preview__toc"><strong>Table of contents</strong><ul>${headings
    .map((heading) => `<li class="depth-${heading.depth}"><a href="#${heading.id}">${heading.title}</a></li>`)
    .join("")}</ul></nav>`;
  let next = html.replace(/\[\[toc\]\]/gi, toc);
  for (const heading of headings) {
    const text = heading.title;
    next = next.replace(
      new RegExp(`<h${heading.depth}>${escapeRegExp(text)}</h${heading.depth}>`),
      `<h${heading.depth} id="${heading.id}">${text}</h${heading.depth}>`,
    );
  }
  return next;
}

export function renderMarkdownPreview(markdown: string): string {
  const withMath = renderMath(markdown);
  const html = marked.parse(withMath, { async: false, gfm: true }) as string;
  return renderToc(markdown, html).replace(
    /<pre><code class="language-mermaid">([\s\S]*?)<\/code><\/pre>/g,
    '<pre class="mermaid">$1</pre>',
  );
}

export function MarkdownPreview({ markdown, className = "markdown-preview", previewRef, onScroll }: MarkdownPreviewProps) {
  const internalPreviewRef = useRef<HTMLDivElement | null>(null);
  const activePreviewRef = previewRef ?? internalPreviewRef;
  const html = useMemo(() => renderMarkdownPreview(markdown), [markdown]);

  useEffect(() => {
    mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
  }, []);

  useEffect(() => {
    if (!activePreviewRef.current) return;
    void mermaid.run({ nodes: activePreviewRef.current.querySelectorAll(".mermaid") });
  }, [activePreviewRef, markdown]);

  return (
    <div
      ref={activePreviewRef}
      className={className}
      onScroll={onScroll}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function imageExtension(type: string): string {
  if (type === "image/jpeg") return "jpg";
  if (type === "image/gif") return "gif";
  if (type === "image/webp") return "webp";
  return "png";
}

function normalizeImageFile(file: File): File {
  if (file.name) return file;
  const ext = imageExtension(file.type);
  return new File([file], `pasted-image-${Date.now()}.${ext}`, { type: file.type });
}

function imageMarkdown(file: File, url: string): string {
  const alt = file.name.replace(/\.[^.]+$/, "") || "image";
  return `![${alt}](${url})`;
}

export function MarkdownEditor({ initialMarkdown, onChange, stickyHeader }: Props) {
  const { message } = App.useApp();
  const [markdown, setMarkdown] = useState(initialMarkdown);
  const [preview, setPreview] = useState(true);
  const [sidePanel, setSidePanel] = useState<"toc" | "detail">("toc");
  const markdownRef = useRef(initialMarkdown);
  const textarea = useRef<HTMLTextAreaElement | null>(null);
  const previewRef = useRef<HTMLDivElement | null>(null);
  const scrollSyncSource = useRef<"editor" | "preview" | null>(null);
  const sourceBlockPositionCache = useRef<{ key: string; positions: HeadingPosition[] } | null>(null);
  const imageInput = useRef<HTMLInputElement | null>(null);
  const attachmentInput = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    markdownRef.current = initialMarkdown;
    setMarkdown(initialMarkdown);
  }, [initialMarkdown]);

  const headings = useMemo(() => extractHeadings(markdown), [markdown]);
  const sourceBlocks = useMemo(() => extractSourceBlocks(markdown), [markdown]);
  const detail = useMemo(
    () => ({
      chars: markdown.length,
      words: markdown.trim() ? markdown.trim().split(/\s+/).length : 0,
      images: (markdown.match(/!\[[^\]]*]\([^)]+\)/g) ?? []).length,
      links: (markdown.match(/(?<!!)\[[^\]]+]\([^)]+\)/g) ?? []).length,
    }),
    [markdown],
  );

  const update = (next: string) => {
    markdownRef.current = next;
    setMarkdown(next);
    onChange(next);
  };

  const replaceText = (from: string, to: string) => {
    const next = markdownRef.current.replace(from, to);
    update(next);
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

  const insertBlock = (value: string) => {
    const el = textarea.current;
    if (!el) return;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const prefix = start > 0 && !markdown.slice(0, start).endsWith("\n") ? "\n\n" : "";
    const suffix = end < markdown.length && !markdown.slice(end).startsWith("\n") ? "\n\n" : "";
    const next = `${markdown.slice(0, start)}${prefix}${value}${suffix}${markdown.slice(end)}`;
    update(next);
    requestAnimationFrame(() => {
      el.focus();
      const cursor = start + prefix.length + value.length;
      el.selectionStart = cursor;
      el.selectionEnd = cursor;
    });
  };

  const uploadFiles = async (files: File[], kind: "image" | "attachment") => {
    if (!files.length) return;
    const hide = message.loading("Uploading...", 0);
    try {
      const snippets: string[] = [];
      for (const file of files) {
        const uploaded = await uploadAttachment(file);
        snippets.push(kind === "image" ? imageMarkdown(file, uploaded.url) : `[${file.name}](${uploaded.url})`);
      }
      insertBlock(snippets.join("\n\n"));
      void message.success(files.length === 1 ? "Uploaded" : "Files uploaded");
    } catch (error) {
      void message.error(error instanceof Error ? error.message : "Upload failed");
    } finally {
      hide();
    }
  };

  const applyLinePrefix = (prefix: string) => {
    const el = textarea.current;
    if (!el) return;
    const start = el.selectionStart;
    const end = el.selectionEnd;
    const lineStart = markdown.lastIndexOf("\n", start - 1) + 1;
    const lineEndIndex = markdown.indexOf("\n", end);
    const lineEnd = lineEndIndex === -1 ? markdown.length : lineEndIndex;
    const block = markdown.slice(lineStart, lineEnd);
    const replacement = block.split("\n").map((line) => `${prefix}${line}`).join("\n");
    update(`${markdown.slice(0, lineStart)}${replacement}${markdown.slice(lineEnd)}`);
  };

  const handlePaste = async (event: ClipboardEvent<HTMLTextAreaElement>) => {
    const files = [...event.clipboardData.files]
      .filter((file) => file.type.startsWith("image/"))
      .map(normalizeImageFile);
    if (!files.length) return;

    event.preventDefault();
    const el = textarea.current;
    const start = el?.selectionStart ?? markdownRef.current.length;
    const end = el?.selectionEnd ?? start;
    const placeholders = files.map((file, index) => {
      const token = `uploading-image-${Date.now()}-${index}`;
      return {
        file,
        token,
        markdown: `![Uploading ${file.name}...](${token})`,
      };
    });
    const insertion = placeholders.map((item) => item.markdown).join("\n\n");
    const next = `${markdownRef.current.slice(0, start)}${insertion}${markdownRef.current.slice(end)}`;
    update(next);

    requestAnimationFrame(() => {
      el?.focus();
      if (el) {
        const cursor = start + insertion.length;
        el.selectionStart = cursor;
        el.selectionEnd = cursor;
      }
    });

    const hide = message.loading("Uploading image...", 0);
    try {
      for (const item of placeholders) {
        const uploaded = await uploadAttachment(item.file);
        replaceText(item.markdown, imageMarkdown(item.file, uploaded.url));
      }
      void message.success(files.length === 1 ? "Image uploaded" : "Images uploaded");
    } catch (error) {
      void message.error(error instanceof Error ? error.message : "Image upload failed");
    } finally {
      hide();
    }
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

  useEffect(() => {
    if (!previewRef.current) return;
    const elements = [
      ...previewRef.current.querySelectorAll<HTMLElement>(
        ":scope > h1, :scope > h2, :scope > h3, :scope > h4, :scope > h5, :scope > h6, :scope > p, :scope > pre, :scope > ul, :scope > ol, :scope > blockquote, :scope > table, :scope > hr",
      ),
    ];
    elements.forEach((element, index) => {
      const block = sourceBlocks[index];
      if (block) {
        element.dataset.sourceLine = String(block.line);
      } else {
        delete element.dataset.sourceLine;
      }
    });
  }, [markdown, sourceBlocks]);

  const editorSourceBlockPositions = (source: HTMLTextAreaElement): HeadingPosition[] => {
    if (!sourceBlocks.length) return [];
    const key = [
      markdown,
      source.clientWidth,
      source.scrollWidth,
      sourceBlocks.map((block) => `${block.line}:${block.offset}`).join("|"),
    ].join("::");
    if (sourceBlockPositionCache.current?.key === key) {
      return sourceBlockPositionCache.current.positions;
    }

    const style = getComputedStyle(source);
    const mirror = document.createElement("div");
    mirror.setAttribute("aria-hidden", "true");
    Object.assign(mirror.style, {
      position: "absolute",
      visibility: "hidden",
      pointerEvents: "none",
      zIndex: "-1",
      left: "-9999px",
      top: "0",
      width: `${source.clientWidth}px`,
      boxSizing: style.boxSizing,
      whiteSpace: "pre-wrap",
      overflowWrap: "break-word",
      wordBreak: style.wordBreak,
      font: style.font,
      fontFamily: style.fontFamily,
      fontSize: style.fontSize,
      fontWeight: style.fontWeight,
      letterSpacing: style.letterSpacing,
      lineHeight: style.lineHeight,
      padding: style.padding,
      border: style.border,
    });

    let cursor = 0;
    for (const block of sourceBlocks) {
      mirror.append(document.createTextNode(markdown.slice(cursor, block.offset)));
      const marker = document.createElement("span");
      marker.dataset.sourceLine = String(block.line);
      marker.style.display = "inline-block";
      marker.style.width = "0";
      marker.style.height = "0";
      marker.style.overflow = "hidden";
      mirror.append(marker);
      cursor = block.offset;
    }
    mirror.append(document.createTextNode(markdown.slice(cursor)));
    document.body.append(mirror);
    const positions = sourceBlocks.map((block) => {
      const marker = mirror.querySelector<HTMLElement>(`[data-source-line="${block.line}"]`);
      return {
        id: String(block.line),
        top: marker?.offsetTop ?? 0,
      };
    });
    mirror.remove();
    sourceBlockPositionCache.current = { key, positions };
    return positions;
  };

  const syncFromEditor = (source: HTMLTextAreaElement) => {
    const target = previewRef.current;
    if (!target || scrollSyncSource.current || !sourceBlocks.length) return;
    const positions = editorSourceBlockPositions(source);
    const activePosition =
      [...positions].reverse().find((position) => position.top <= source.scrollTop + 8) ??
      positions[0];
    if (!activePosition) return;
    const targetBlock = target.querySelector<HTMLElement>(`[data-source-line="${activePosition.id}"]`);
    if (!targetBlock) return;
    scrollSyncSource.current = "editor";
    target.scrollTop = Math.max(0, targetBlock.offsetTop - 8);
    requestAnimationFrame(() => {
      scrollSyncSource.current = null;
    });
  };

  const syncFromPreview = (source: HTMLDivElement) => {
    if (!textarea.current || scrollSyncSource.current || !sourceBlocks.length) return;
    const elements = [...source.querySelectorAll<HTMLElement>("[data-source-line]")];
    if (!elements.length) return;
    const scrollTop = source.scrollTop + 12;
    const activeElement = [...elements].reverse().find((element) => element.offsetTop <= scrollTop) ?? elements[0];
    if (!activeElement) return;
    const line = activeElement.dataset.sourceLine;
    if (!line) return;
    const positions = editorSourceBlockPositions(textarea.current);
    const position = positions.find((item) => item.id === line);
    if (!position) return;
    scrollSyncSource.current = "preview";
    textarea.current.scrollTop = position.top;
    requestAnimationFrame(() => {
      scrollSyncSource.current = null;
    });
  };

  return (
    <div className="markdown-editor-shell">
      <div className="markdown-editor-sticky-top">
        {stickyHeader}
        <div className="lexical-toolbar">
          <Space size={4} wrap>
            <Tooltip title="Undo"><Button size="small" icon={<UndoOutlined />} onClick={() => document.execCommand("undo")} /></Tooltip>
            <Tooltip title="Redo"><Button size="small" icon={<RedoOutlined />} onClick={() => document.execCommand("redo")} /></Tooltip>
            <Tooltip title="Bold"><Button size="small" icon={<BoldOutlined />} onClick={() => insert("**", "**", "bold")} /></Tooltip>
            <Tooltip title="Italic"><Button size="small" icon={<ItalicOutlined />} onClick={() => insert("_", "_", "italic")} /></Tooltip>
            <Tooltip title="Underline"><Button size="small" onClick={() => insert("<u>", "</u>", "underline")}>U</Button></Tooltip>
            <Tooltip title="Strikethrough"><Button size="small" icon={<StrikethroughOutlined />} onClick={() => insert("~~", "~~", "deleted")} /></Tooltip>
            <Tooltip title="Highlight"><Button size="small" onClick={() => insert("<mark>", "</mark>", "mark")}>Mark</Button></Tooltip>
            <Tooltip title="Inline code"><Button size="small" icon={<CodeOutlined />} onClick={() => insert("`", "`", "code")} /></Tooltip>
            <Tooltip title="Quote"><Button size="small" onClick={() => applyLinePrefix("> ")}>Quote</Button></Tooltip>
            <Dropdown
              menu={{
                items: [
                  { key: "h1", label: "Heading 1", onClick: () => applyLinePrefix("# ") },
                  { key: "h2", label: "Heading 2", onClick: () => applyLinePrefix("## ") },
                  { key: "h3", label: "Heading 3", onClick: () => applyLinePrefix("### ") },
                  { key: "hr", label: "Divider", onClick: () => insertBlock("---") },
                ],
              }}
            >
              <Button size="small">H</Button>
            </Dropdown>
            <Tooltip title="Bullet list"><Button size="small" icon={<UnorderedListOutlined />} onClick={() => insert("- ", "", "list item")} /></Tooltip>
            <Tooltip title="Numbered list"><Button size="small" icon={<OrderedListOutlined />} onClick={() => insert("1. ", "", "list item")} /></Tooltip>
            <Tooltip title="Link"><Button size="small" icon={<LinkOutlined />} onClick={() => insert("[", "](https://)", "link text")} /></Tooltip>
            <Tooltip title="Image"><Button size="small" icon={<PictureOutlined />} onClick={() => imageInput.current?.click()} /></Tooltip>
            <Tooltip title="Table"><Button size="small" icon={<TableOutlined />} onClick={() => insert("\n| Column | Column |\n| --- | --- |\n| ", " | value |\n", "value")} /></Tooltip>
            <Tooltip title="Clear format"><Button size="small" icon={<ClearOutlined />} onClick={clearFormat} /></Tooltip>
            <Dropdown
              menu={{
                items: [
                  { key: "attachment", label: "Attachment", icon: <UploadOutlined />, onClick: () => attachmentInput.current?.click() },
                  { key: "table", label: "Add table", onClick: () => insertBlock("| Column | Column |\n| --- | --- |\n| value | value |") },
                  { key: "video", label: "Video", icon: <VideoCameraOutlined />, onClick: () => insertBlock('<video controls src="https://example.com/video.mp4"></video>') },
                  { key: "audio", label: "Audio", icon: <AudioOutlined />, onClick: () => insertBlock('<audio controls src="https://example.com/audio.mp3"></audio>') },
                  { key: "iframe", label: "Iframe", onClick: () => insertBlock('<iframe src="https://example.com" width="100%" height="360"></iframe>') },
                  { key: "detail", label: "Detail block", onClick: () => insert("\n<details>\n<summary>", "</summary>\n\nDetail content\n</details>\n", "Title") },
                  { key: "columns", label: "Column Card", onClick: () => insertBlock('<div class="columns">\n\n<div>\n\nColumn 1\n\n</div>\n\n<div>\n\nColumn 2\n\n</div>\n\n</div>') },
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
          <input
            ref={imageInput}
            type="file"
            accept="image/*"
            multiple
            hidden
            onChange={(event) => {
              void uploadFiles([...(event.target.files ?? [])], "image");
              event.currentTarget.value = "";
            }}
          />
          <input
            ref={attachmentInput}
            type="file"
            multiple
            hidden
            onChange={(event) => {
              void uploadFiles([...(event.target.files ?? [])], "attachment");
              event.currentTarget.value = "";
            }}
          />
        </div>
      </div>
      <div className={`markdown-editor-grid${preview ? "" : " markdown-editor-grid--single"}`}>
        <textarea
          ref={textarea}
          className="markdown-editor-input"
          aria-label="Post body"
          value={markdown}
          onChange={(event) => update(event.target.value)}
          onScroll={(event) => syncFromEditor(event.currentTarget)}
          onPaste={handlePaste}
          placeholder="Write your post in markdown..."
        />
        {preview ? (
          <MarkdownPreview
            markdown={markdown}
            previewRef={previewRef}
            onScroll={(event) => syncFromPreview(event.currentTarget)}
          />
        ) : null}
        {preview ? (
          <aside className="markdown-inspector">
            <div className="markdown-inspector__tabs">
              <button type="button" className={sidePanel === "toc" ? "active" : ""} onClick={() => setSidePanel("toc")}>Toc</button>
              <button type="button" className={sidePanel === "detail" ? "active" : ""} onClick={() => setSidePanel("detail")}>Detail</button>
            </div>
            {sidePanel === "toc" ? (
              <div className="markdown-inspector__body">
                {headings.length ? (
                  <ul className="markdown-inspector__toc">
                    {headings.map((heading, index) => (
                      <li key={`${heading.id}-${index}`} className={`depth-${heading.depth}`}>{heading.title}</li>
                    ))}
                  </ul>
                ) : (
                  <p className="markdown-inspector__empty">No Toc available</p>
                )}
              </div>
            ) : (
              <dl className="markdown-inspector__detail">
                <div><dt>Characters</dt><dd>{detail.chars}</dd></div>
                <div><dt>Words</dt><dd>{detail.words}</dd></div>
                <div><dt>Images</dt><dd>{detail.images}</dd></div>
                <div><dt>Links</dt><dd>{detail.links}</dd></div>
              </dl>
            )}
          </aside>
        ) : null}
      </div>
    </div>
  );
}
