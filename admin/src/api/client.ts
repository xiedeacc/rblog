// Thin fetch wrapper for the rblog admin REST API.
//
// Sessions are cookie-based, so every request must include credentials.
// On 401/403 we redirect to /admin/login. All other non-2xx responses
// throw `ApiError` so React Query / handlers can react to them.
//
// The shapes here are kept in lock-step with the `#[derive(Serialize)]`
// types in `crates/rblog-http/src/routes/admin/*.rs`. If you change one,
// change the other. `pnpm gen:api` can regenerate `schema.ts` from the
// live `/api/admin/openapi.json` for type-only verification.

import type { AuthUser } from "@/state/auth";

export class ApiError extends Error {
  readonly status: number;
  readonly body: unknown;

  constructor(status: number, message: string, body: unknown) {
    super(message);
    this.status = status;
    this.body = body;
  }
}

export interface PostSummary {
  name: string;
  title: string;
  slug: string;
  permalink: string;
  publish_time: string | null;
  published: boolean;
  visible: string;
  deleted: boolean;
  deletion_time: string | null;
  creation_time: string | null;
  last_modify_time: string | null;
  comments_count: number;
  visits: number;
  tags: string[];
  categories: string[];
}

export interface ListPage<T> {
  items: T[];
  total: number;
}

export interface PostDetail {
  name: string;
  title: string;
  slug: string;
  permalink: string;
  content_html: string;
  raw_markdown: string;
  excerpt: string;
  publish_time: string | null;
  published: boolean;
  deleted: boolean;
  visible: string;
  owner: string | null;
  categories: string[];
  tags: string[];
  cover: string | null;
  template: string | null;
  pinned: boolean;
  allow_comment: boolean;
  priority: number;
}

export interface CommentItem {
  name: string;
  kind: "Comment" | "Reply";
  raw: string;
  content: string;
  owner_name: string;
  owner_kind: string;
  owner_display: string;
  subject_kind: string;
  subject_name: string;
  parent_name: string | null;
  created_at: string | null;
  approved: boolean;
  hidden: boolean;
}

export interface UserItem {
  name: string;
  display_name: string;
  email: string;
  disabled: boolean;
  registered_at: string | null;
}

export interface AttachmentListItem {
  key: string;
  url: string;
  size: number;
}

export interface ThumbnailItem {
  name: string;
  url: string;
  key: string;
  width: number;
  height: number;
}

export interface UploadResponse {
  key: string;
  url: string;
  media_type: string | null;
  size: number;
  thumbnails: ThumbnailItem[];
}

export interface SystemInfo {
  version: string;
  active_theme: string;
  active_theme_directory: string;
  themes: string[];
}

export interface ConfigMapView {
  name: string;
  data: Record<string, string>;
  version: number | null;
}

async function request<T>(
  path: string,
  init: RequestInit & { json?: unknown; noAuthRedirect?: boolean } = {},
): Promise<T> {
  const headers = new Headers(init.headers);
  if (init.json !== undefined) {
    headers.set("Content-Type", "application/json");
  }
  const res = await fetch(path, {
    ...init,
    credentials: "same-origin",
    headers,
    body: init.json !== undefined ? JSON.stringify(init.json) : init.body,
  });
  const text = await res.text();
  let body: unknown = null;
  if (text.length > 0) {
    try {
      body = JSON.parse(text);
    } catch {
      body = text;
    }
  }
  if (!res.ok) {
    const message = errorMessage(body) ?? `${res.status} ${res.statusText}`;
    if (!init.noAuthRedirect && (res.status === 401 || res.status === 403)) {
      if (typeof window !== "undefined" && !window.location.pathname.endsWith("/login")) {
        window.location.assign("/admin/login");
      }
    }
    throw new ApiError(res.status, String(message), body);
  }
  return body as T;
}

function errorMessage(body: unknown): string | null {
  if (typeof body === "string") return body;
  if (!body || typeof body !== "object" || !("error" in body)) return null;

  const error = (body as { error: unknown }).error;
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message: unknown }).message;
    if (typeof message === "string") return message;
  }
  return null;
}

// ────────────────────────────────────── auth + system

export const fetchWhoAmI = () =>
  request<AuthUser>("/api/admin/auth/session", { noAuthRedirect: true });

export const login = (username: string, password: string) =>
  request<{ name: string; display_name: string; email: string }>(
    "/api/admin/auth/login",
    { method: "POST", json: { username, password } },
  );

export const logout = () =>
  request<void>("/api/admin/auth/logout", { method: "POST" });

export const fetchSystemInfo = () =>
  request<SystemInfo>("/api/admin/system/info");

export const fetchBootstrapStatus = () =>
  request<{ bootstrapped: boolean }>("/api/admin/bootstrap/status", {
    noAuthRedirect: true,
  });

export const rebuildSearchIndex = () =>
  request<{ indexed: number }>("/api/admin/system/search/rebuild", {
    method: "POST",
  });

export interface RestoreHaloDumpResponse {
  restored_rows: number;
  posts: number;
  snapshots: number;
  tags: number;
  categories: number;
  comments: number;
  users: number;
  search_indexed: number;
}

export const restoreHaloDump = (path: string) =>
  request<RestoreHaloDumpResponse>("/api/admin/system/restore/halo", {
    method: "POST",
    json: { path },
  });

export interface BootstrapRequest {
  admin_username: string;
  admin_email?: string;
  admin_password: string;
  site_title?: string;
  site_subtitle?: string;
  site_base_url?: string;
}

export const bootstrap = (req: BootstrapRequest) =>
  request<{ bootstrapped: boolean }>("/api/admin/bootstrap", {
    method: "POST",
    json: req,
  });

// ────────────────────────────────────── posts

export const listPosts = (
  params: {
    offset?: number;
    limit?: number;
    status?: string;
    tag?: string;
    category?: string;
    include_deleted?: boolean;
    deleted_only?: boolean;
    visible?: string;
  } = {},
) => {
  const qp = new URLSearchParams();
  if (params.offset !== undefined) qp.set("offset", String(params.offset));
  if (params.limit !== undefined) qp.set("limit", String(params.limit));
  if (params.status) qp.set("status", params.status);
  if (params.tag) qp.set("tag", params.tag);
  if (params.category) qp.set("category", params.category);
  if (params.include_deleted) qp.set("include_deleted", "true");
  if (params.deleted_only) qp.set("deleted_only", "true");
  if (params.visible) qp.set("visible", params.visible);
  const qs = qp.toString();
  return request<ListPage<PostSummary>>(`/api/admin/posts${qs ? `?${qs}` : ""}`);
};

export const fetchPost = (name: string) =>
  request<PostDetail>(`/api/admin/posts/${encodeURIComponent(name)}`);

export interface CreatePostBody {
  name: string;
  title: string;
  slug: string;
  markdown: string;
  tags?: string[];
  categories?: string[];
  cover?: string;
  template?: string;
  excerpt?: string;
  priority?: number;
  pinned?: boolean;
  allow_comment?: boolean;
  publish_time?: string | null;
  visible?: string;
}

export const createPost = (body: CreatePostBody) =>
  request<PostDetail>("/api/admin/posts", { method: "POST", json: body });

export interface UpdatePostBody {
  markdown: string;
  title?: string;
  slug?: string;
  excerpt?: string;
  visible?: string;
  cover?: string;
  template?: string;
  priority?: number;
  pinned?: boolean;
  allow_comment?: boolean;
  publish_time?: string | null;
}

export const updatePostContent = (name: string, body: UpdatePostBody) =>
  request<PostDetail>(`/api/admin/posts/${encodeURIComponent(name)}`, {
    method: "PUT",
    json: body,
  });

export interface PublishBody {
  visible?: string;
  publish_time?: string;
}
export const publishPost = (name: string, body: PublishBody = {}) =>
  request<PostDetail>(`/api/admin/posts/${encodeURIComponent(name)}/publish`, {
    method: "POST",
    json: body,
  });

export const unpublishPost = (name: string) =>
  request<PostDetail>(`/api/admin/posts/${encodeURIComponent(name)}/unpublish`, {
    method: "POST",
  });

export const restorePost = (name: string) =>
  request<PostDetail>(`/api/admin/posts/${encodeURIComponent(name)}/restore`, {
    method: "POST",
  });

export const softDeletePost = (name: string) =>
  request<void>(`/api/admin/posts/${encodeURIComponent(name)}`, {
    method: "DELETE",
  });

export const purgePost = (name: string) =>
  request<void>(`/api/admin/posts/${encodeURIComponent(name)}/purge`, {
    method: "DELETE",
  });

// ────────────────────────────────────── comments

export const listCommentQueue = () =>
  request<CommentItem[]>("/api/admin/comments/queue");
export const listComments = (params: { status?: string; kind?: string } = {}) => {
  const qp = new URLSearchParams();
  if (params.status) qp.set("status", params.status);
  if (params.kind) qp.set("kind", params.kind);
  const qs = qp.toString();
  return request<CommentItem[]>(`/api/admin/comments${qs ? `?${qs}` : ""}`);
};
export const approveComment = (name: string) =>
  request<unknown>(`/api/admin/comments/${encodeURIComponent(name)}/approve`, {
    method: "POST",
  });
export const hideComment = (name: string) =>
  request<unknown>(`/api/admin/comments/${encodeURIComponent(name)}/hide`, {
    method: "POST",
  });
export const showComment = (name: string) =>
  request<unknown>(`/api/admin/comments/${encodeURIComponent(name)}/show`, {
    method: "POST",
  });
export const deleteComment = (name: string) =>
  request<void>(`/api/admin/comments/${encodeURIComponent(name)}`, { method: "DELETE" });
export const approveReply = (name: string) =>
  request<unknown>(`/api/admin/replies/${encodeURIComponent(name)}/approve`, {
    method: "POST",
  });
export const hideReply = (name: string) =>
  request<unknown>(`/api/admin/replies/${encodeURIComponent(name)}/hide`, {
    method: "POST",
  });
export const showReply = (name: string) =>
  request<unknown>(`/api/admin/replies/${encodeURIComponent(name)}/show`, {
    method: "POST",
  });
export const deleteReply = (name: string) =>
  request<void>(`/api/admin/replies/${encodeURIComponent(name)}`, { method: "DELETE" });

// ────────────────────────────────────── users

export const listUsers = () => request<UserItem[]>("/api/admin/users");
export const createUser = (body: {
  name: string;
  email: string;
  display_name: string;
  password: string;
}) => request<UserItem>("/api/admin/users", { method: "POST", json: body });
export const fetchUser = (name: string) =>
  request<UserItem>(`/api/admin/users/${encodeURIComponent(name)}`);
export const setUserPassword = (name: string, password: string) =>
  request<void>(`/api/admin/users/${encodeURIComponent(name)}/password`, {
    method: "PUT",
    json: { password },
  });
export const disableUser = (name: string) =>
  request<UserItem>(`/api/admin/users/${encodeURIComponent(name)}/disable`, {
    method: "POST",
  });
export const enableUser = (name: string) =>
  request<UserItem>(`/api/admin/users/${encodeURIComponent(name)}/enable`, {
    method: "POST",
  });
export const removeUser = (name: string) =>
  request<void>(`/api/admin/users/${encodeURIComponent(name)}`, { method: "DELETE" });

// ────────────────────────────────────── attachments

export const listAttachments = (prefix?: string) => {
  const qs = prefix ? `?prefix=${encodeURIComponent(prefix)}` : "";
  return request<AttachmentListItem[]>(`/api/admin/attachments${qs}`);
};
export const uploadAttachment = (file: File, group?: string) => {
  const fd = new FormData();
  fd.append("file", file);
  if (group) fd.append("group", group);
  return fetch("/api/admin/attachments", {
    method: "POST",
    body: fd,
    credentials: "same-origin",
  }).then(async (res) => {
    if (!res.ok) throw new ApiError(res.status, res.statusText, await res.text());
    return (await res.json()) as UploadResponse;
  });
};
export const removeAttachment = (key: string) =>
  request<void>(`/api/admin/attachments/${key.split("/").map(encodeURIComponent).join("/")}`, {
    method: "DELETE",
  });

// ────────────────────────────────────── settings

export const fetchConfigMap = (name: string) =>
  request<ConfigMapView>(`/api/admin/configmaps/${encodeURIComponent(name)}`);
export const upsertConfigMap = (name: string, data: Record<string, string>) =>
  request<ConfigMapView>(`/api/admin/configmaps/${encodeURIComponent(name)}`, {
    method: "PUT",
    json: { data },
  });

export const fetchSystemSettings = () =>
  request<ConfigMapView>("/api/admin/system/settings");
export const upsertSystemSettings = (data: Record<string, string>) =>
  request<ConfigMapView>("/api/admin/system/settings", {
    method: "PUT",
    json: { data },
  });

// ────────────────────────────────────── plugins

export interface PluginRoute {
  path: string;
  methods: string[];
}

export interface PluginInfo {
  name: string;
  display_name: string;
  version: string;
  description: string | null;
  authors: string[];
  enabled: boolean;
  capabilities: string[];
  routes: PluginRoute[];
  directory: string;
  entry: string;
}

export const listPlugins = () =>
  request<{ plugins: PluginInfo[] }>("/api/admin/plugins").then((r) => r.plugins);
export const enablePlugin = (name: string) =>
  request<PluginInfo>(`/api/admin/plugins/${encodeURIComponent(name)}/enable`, {
    method: "POST",
  });
export const disablePlugin = (name: string) =>
  request<PluginInfo>(`/api/admin/plugins/${encodeURIComponent(name)}/disable`, {
    method: "POST",
  });
export const reloadPlugins = () =>
  request<{ loaded: number }>("/api/admin/plugins/reload", { method: "POST" });
